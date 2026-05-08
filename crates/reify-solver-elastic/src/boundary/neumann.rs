//! Neumann boundary condition application for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #11.
//!
//! # Additive accumulation contract
//!
//! All three `apply_*` primitives add (`+=`) into the caller-supplied
//! `&mut [f64]` load vector rather than overwriting it. This means multiple
//! loads can be applied sequentially to the same `f` and the contributions
//! compose correctly:
//!
//! ```text
//! let mut f = vec![0.0; 3 * n_nodes];
//! for load in &surface_tractions { apply_traction_load(&mut f, ...) }
//! for load in &body_forces       { apply_body_force(&mut f, ...)    }
//! for load in &point_loads       { apply_point_load(&mut f, ...)    }
//! // f now holds ∑ all contributions
//! ```
//!
//! The DOF ordering is `f[3 * node_idx + axis]` for `axis ∈ {0, 1, 2}` —
//! matching the `3 * conn[a] + α` mapping used by
//! [`crate::assembly::assemble_global_stiffness`].

use crate::assembly::ElementOrder;
use crate::elements::ReferenceElement;
use crate::elements::tet_p1::TetP1;
use crate::elements::tet_p2::TetP2;

/// Triangular face interpolation order.
///
/// Distinguishes 3-node linear faces (`P1Tri`) from 6-node quadratic faces
/// (`P2Tri`) of tetrahedral elements. Separate from
/// [`crate::assembly::ElementOrder`] (which keys on volume-element node count)
/// because surface tractions key on face node count — reusing `ElementOrder`
/// would invite confusion at call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceOrder {
    /// 3-node linear triangular face (P1 tet face).
    P1Tri,
    /// 6-node quadratic triangular face (P2 tet face): 3 vertices followed by
    /// 3 edge-midpoints in canonical order `[v0, v1, v2, m_{01}, m_{12}, m_{20}]`.
    P2Tri,
}

// ---------------------------------------------------------------------------
// apply_point_load
// ---------------------------------------------------------------------------

/// Apply a concentrated nodal force at a single node of the global load vector.
///
/// Adds `force[α]` to `f[3 * node + α]` for each axis `α ∈ {0, 1, 2}`.
///
/// # Additive semantics
///
/// This function uses `+=`, not `=`. Multiple calls with the same `node`
/// accumulate correctly. See the module-level doc for the composition contract.
///
/// # Panics
///
/// - `f.len() % 3 != 0` — the load vector length must be a multiple of 3
///   (one entry per axis per node, Task-2544 contract-explicitness convention).
/// - `3 * node + 2 >= f.len()` — the node index is out of range for the given
///   load vector.
pub fn apply_point_load(f: &mut [f64], node: usize, force: [f64; 3]) {
    assert!(
        f.len().is_multiple_of(3),
        "apply_point_load: f.len() = {} is not a multiple of 3; \
         the global load vector must have exactly 3 DOFs per node",
        f.len(),
    );
    assert!(
        3 * node + 2 < f.len(),
        "apply_point_load: node {} is out of range for f.len() = {}; \
         valid node indices are 0..{}",
        node,
        f.len(),
        f.len() / 3,
    );
    for alpha in 0..3 {
        f[3 * node + alpha] += force[alpha];
    }
}

// ---------------------------------------------------------------------------
// apply_body_force (generic helper + public dispatcher)
// ---------------------------------------------------------------------------

/// Generic body-force integrator over a single tetrahedral element.
///
/// Computes `∫_Ω N_i(x) dV` for each node `i` using volume quadrature
/// supplied by the element `E`, then scatters
/// `f[3 * connectivity[i] + α] += w_i * body_force[α]`.
///
/// The per-node weights `w_i = Σ_q N_i(q.coord) · |det J(q)| · q.weight`
/// include `|det J(q)|` so the total body-force scales linearly with element
/// physical volume: a uniform 2× scale in all three axes gives 8× volume →
/// 8× per-node contribution, matching the continuum expectation.
///
/// # Panics (unconditional, Task-2544 contract-explicitness convention)
///
/// - `connectivity.len() != E::N_NODES`
/// - `phys_nodes.len() != E::N_NODES`
/// - `f.len() % 3 != 0`
/// - Any entry in `connectivity` is `>= f.len() / 3` (out-of-range global node)
fn integrate_body_force_generic<E: ReferenceElement>(
    element: &E,
    f: &mut [f64],
    connectivity: &[usize],
    phys_nodes: &[[f64; 3]],
    body_force: [f64; 3],
) {
    assert_eq!(
        connectivity.len(),
        E::N_NODES,
        "integrate_body_force_generic: connectivity.len() = {} but expected {} (E::N_NODES) \
         for this element order",
        connectivity.len(),
        E::N_NODES,
    );
    assert_eq!(
        phys_nodes.len(),
        E::N_NODES,
        "integrate_body_force_generic: phys_nodes.len() = {} but expected {} (E::N_NODES) \
         for this element order",
        phys_nodes.len(),
        E::N_NODES,
    );
    assert!(
        f.len().is_multiple_of(3),
        "integrate_body_force_generic: f.len() = {} is not a multiple of 3; \
         the global load vector must have exactly 3 DOFs per node",
        f.len(),
    );
    let n_global_nodes = f.len() / 3;
    for (local_i, &global_node) in connectivity.iter().enumerate() {
        assert!(
            global_node < n_global_nodes,
            "integrate_body_force_generic: connectivity[{}] = {} is out of range; \
             f.len() / 3 = {} global nodes",
            local_i,
            global_node,
            n_global_nodes,
        );
    }

    // Accumulate per-node integration weights w_i = Σ_q N_i(q) · |det J(q)| · q.weight.
    let mut nodal_weights = vec![0.0_f64; E::N_NODES];
    for qp in element.quad_points() {
        let shapes = element.shape_at(qp.coord);
        let jac = element.jacobian(phys_nodes, qp.coord);
        let factor = jac.det.abs() * qp.weight;
        for i in 0..E::N_NODES {
            nodal_weights[i] += shapes[i] * factor;
        }
    }

    // Scatter into global f.
    for (i, &global_node) in connectivity.iter().enumerate() {
        for alpha in 0..3 {
            f[3 * global_node + alpha] += nodal_weights[i] * body_force[alpha];
        }
    }
}

/// Apply a uniform body force over a single tetrahedral element.
///
/// Computes `∫_Ω N^T b dV` via volume quadrature and accumulates the result
/// into the global load vector `f`. The integral is dispatched through
/// [`crate::elements::ReferenceElement`] shape functions and the element's
/// Gauss quadrature rule:
///
/// - `ElementOrder::P1` → `TetP1` (4 nodes, 1-point centroid rule, degree-1
///   exact — sufficient for the `N · const` integrand).
/// - `ElementOrder::P2` → `TetP2` (10 nodes, 4-point Stroud rule, degree-2
///   exact — sufficient for the quadratic `N · const` integrand).
///
/// Per-node weights include `|det J(q)|`, so the total applied force scales
/// linearly with physical element volume (documented in
/// `integrate_body_force_generic`).
///
/// # Additive semantics
///
/// Uses `+=`; multiple body-force calls accumulate correctly. See module doc.
///
/// # Panics
///
/// - `connectivity.len()` does not match the expected node count for `order`
///   (4 for P1, 10 for P2).
/// - `phys_nodes.len()` does not match.
/// - `f.len() % 3 != 0`.
/// - Any connectivity entry is `>= f.len() / 3`.
pub fn apply_body_force(
    f: &mut [f64],
    order: ElementOrder,
    connectivity: &[usize],
    phys_nodes: &[[f64; 3]],
    body_force: [f64; 3],
) {
    match order {
        ElementOrder::P1 => {
            integrate_body_force_generic(&TetP1, f, connectivity, phys_nodes, body_force)
        }
        ElementOrder::P2 => {
            integrate_body_force_generic(&TetP2, f, connectivity, phys_nodes, body_force)
        }
    }
}

// ---------------------------------------------------------------------------
// apply_traction_load (triangle quadrature + public dispatcher)
// ---------------------------------------------------------------------------

/// A 2D reference coordinate `(ξ, η)` on a reference triangle.
///
/// The reference triangle has vertices at `(0,0), (1,0), (0,1)`.
#[derive(Clone, Copy)]
struct TriRefCoord {
    xi: f64,
    eta: f64,
}

/// A quadrature point on the reference triangle.
#[derive(Clone, Copy)]
struct TriQuadPoint {
    coord: TriRefCoord,
    weight: f64,
}

/// 1-point centroid rule for the unit reference triangle (degree-1 exact).
///
/// Point at `(1/3, 1/3)`, weight `1/2` (= reference-triangle area).
const TRI_P1_QUAD: &[TriQuadPoint] = &[TriQuadPoint {
    coord: TriRefCoord {
        xi: 1.0 / 3.0,
        eta: 1.0 / 3.0,
    },
    weight: 0.5,
}];

/// 3-point edge-midpoint rule for the unit reference triangle (degree-2 exact).
///
/// Points at the midpoints of the three edges: `(1/2, 0)`, `(1/2, 1/2)`,
/// `(0, 1/2)`, each with weight `1/6`. Total weight `1/2` = triangle area.
const TRI_P2_QUAD: &[TriQuadPoint] = &[
    TriQuadPoint {
        coord: TriRefCoord { xi: 0.5, eta: 0.0 },
        weight: 1.0 / 6.0,
    },
    TriQuadPoint {
        coord: TriRefCoord { xi: 0.5, eta: 0.5 },
        weight: 1.0 / 6.0,
    },
    TriQuadPoint {
        coord: TriRefCoord { xi: 0.0, eta: 0.5 },
        weight: 1.0 / 6.0,
    },
];

/// P1 triangle shape functions `[N_0, N_1, N_2]` at a reference coordinate.
///
/// `N_0 = 1 - ξ - η`, `N_1 = ξ`, `N_2 = η`.
fn tri_p1_shape(c: TriRefCoord) -> [f64; 3] {
    [1.0 - c.xi - c.eta, c.xi, c.eta]
}

/// P1 triangle shape-function gradients (constant): `[∂N_i/∂ξ, ∂N_i/∂η]`.
///
/// `∇N_0 = (-1, -1)`, `∇N_1 = (1, 0)`, `∇N_2 = (0, 1)`.
const TRI_P1_GRADS: [[f64; 2]; 3] = [[-1.0, -1.0], [1.0, 0.0], [0.0, 1.0]];

/// P2 triangle shape functions (6-node) at a reference coordinate.
///
/// Canonical node order: `[v0, v1, v2, m_{01}, m_{12}, m_{20}]` where
/// `λ_0 = 1-ξ-η`, `λ_1 = ξ`, `λ_2 = η`.
///
/// - Vertex shapes: `λ_i (2 λ_i - 1)`
/// - Edge-midpoint shapes: `4 λ_a λ_b`
///
/// Follows the standard quadratic Lagrangian Serendipity ordering.
fn tri_p2_shape(c: TriRefCoord) -> [f64; 6] {
    let xi = c.xi;
    let eta = c.eta;
    let l0 = 1.0 - xi - eta;
    let l1 = xi;
    let l2 = eta;
    [
        l0 * (2.0 * l0 - 1.0), // N_0
        l1 * (2.0 * l1 - 1.0), // N_1
        l2 * (2.0 * l2 - 1.0), // N_2
        4.0 * l0 * l1,          // N_3 (edge 01 midpoint)
        4.0 * l1 * l2,          // N_4 (edge 12 midpoint)
        4.0 * l0 * l2,          // N_5 (edge 20 midpoint)
    ]
}

/// P2 triangle shape-function gradients `[∂N_i/∂ξ, ∂N_i/∂η]`.
fn tri_p2_grads(c: TriRefCoord) -> [[f64; 2]; 6] {
    let xi = c.xi;
    let eta = c.eta;
    let l0 = 1.0 - xi - eta;
    [
        // ∂N_0/∂ξ = (2(1-ξ-η) - 1)(-1) + (1-ξ-η)(-2)(-1) — computed via product rule
        // N_0 = l0(2l0-1), ∂N_0/∂ξ = ∂l0/∂ξ(2l0-1) + l0·2·∂l0/∂ξ = (-1)(2l0-1) + l0·2·(-1)
        //      = -(2l0-1) - 2l0 = -4l0 + 1
        [-4.0 * l0 + 1.0, -4.0 * l0 + 1.0],
        // N_1 = ξ(2ξ-1), ∂N_1/∂ξ = 4ξ-1, ∂N_1/∂η = 0
        [4.0 * xi - 1.0, 0.0],
        // N_2 = η(2η-1), ∂N_2/∂ξ = 0, ∂N_2/∂η = 4η-1
        [0.0, 4.0 * eta - 1.0],
        // N_3 = 4l0·ξ = 4(1-ξ-η)ξ, ∂N_3/∂ξ = 4(1-2ξ-η), ∂N_3/∂η = -4ξ
        [4.0 * (1.0 - 2.0 * xi - eta), -4.0 * xi],
        // N_4 = 4ξη, ∂N_4/∂ξ = 4η, ∂N_4/∂η = 4ξ
        [4.0 * eta, 4.0 * xi],
        // N_5 = 4l0·η = 4(1-ξ-η)η, ∂N_5/∂ξ = -4η, ∂N_5/∂η = 4(1-ξ-2η)
        [-4.0 * eta, 4.0 * (1.0 - xi - 2.0 * eta)],
    ]
}

/// 3D cross product of two vectors.
#[inline]
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Euclidean norm of a 3D vector.
#[inline]
fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Apply a uniform surface traction over a single triangular face.
///
/// Computes `∫_Γ N^T t dA` via triangle quadrature, using the surface area
/// element `|∂x/∂ξ × ∂x/∂η|` built from the face's physical-node coordinates.
/// Accumulates the result into `f`.
///
/// - `FaceOrder::P1Tri` — 3-node linear triangle, 1-point centroid quadrature
///   (degree-1 exact for the `N · const` integrand).
/// - `FaceOrder::P2Tri` — 6-node quadratic triangle in canonical order
///   `[v0, v1, v2, m_{01}, m_{12}, m_{20}]`, 3-point edge-midpoint quadrature
///   (degree-2 exact).
///
/// # Additive semantics
///
/// Uses `+=`; multiple traction calls accumulate correctly. See module doc.
///
/// # Panics
///
/// - `connectivity.len()` does not match the face node count (3 for P1Tri,
///   6 for P2Tri).
/// - `phys_nodes.len()` does not match.
/// - `f.len() % 3 != 0`.
/// - Any connectivity entry is `>= f.len() / 3`.
pub fn apply_traction_load(
    f: &mut [f64],
    face_order: FaceOrder,
    connectivity: &[usize],
    phys_nodes: &[[f64; 3]],
    traction: [f64; 3],
) {
    let n_face_nodes = match face_order {
        FaceOrder::P1Tri => 3,
        FaceOrder::P2Tri => 6,
    };

    assert_eq!(
        connectivity.len(),
        n_face_nodes,
        "apply_traction_load: connectivity.len() = {} but expected {} face nodes for {:?}",
        connectivity.len(),
        n_face_nodes,
        face_order,
    );
    assert_eq!(
        phys_nodes.len(),
        n_face_nodes,
        "apply_traction_load: phys_nodes.len() = {} but expected {} face nodes for {:?}",
        phys_nodes.len(),
        n_face_nodes,
        face_order,
    );
    assert!(
        f.len().is_multiple_of(3),
        "apply_traction_load: f.len() = {} is not a multiple of 3; \
         the global load vector must have exactly 3 DOFs per node",
        f.len(),
    );
    let n_global_nodes = f.len() / 3;
    for (local_i, &global_node) in connectivity.iter().enumerate() {
        assert!(
            global_node < n_global_nodes,
            "apply_traction_load: connectivity[{}] = {} is out of range; \
             f.len() / 3 = {} global nodes",
            local_i,
            global_node,
            n_global_nodes,
        );
    }

    let mut nodal_weights = vec![0.0_f64; n_face_nodes];

    match face_order {
        FaceOrder::P1Tri => {
            for qp in TRI_P1_QUAD {
                let shapes = tri_p1_shape(qp.coord);
                // Tangent vectors: t_ξ = Σ_i (∂N_i/∂ξ) · phys_nodes[i]
                let mut t_xi = [0.0_f64; 3];
                let mut t_eta = [0.0_f64; 3];
                for i in 0..3 {
                    for d in 0..3 {
                        t_xi[d] += TRI_P1_GRADS[i][0] * phys_nodes[i][d];
                        t_eta[d] += TRI_P1_GRADS[i][1] * phys_nodes[i][d];
                    }
                }
                let area_elem = norm3(cross(t_xi, t_eta));
                for i in 0..3 {
                    nodal_weights[i] += shapes[i] * area_elem * qp.weight;
                }
            }
        }
        FaceOrder::P2Tri => {
            for qp in TRI_P2_QUAD {
                let shapes = tri_p2_shape(qp.coord);
                let grads = tri_p2_grads(qp.coord);
                let mut t_xi = [0.0_f64; 3];
                let mut t_eta = [0.0_f64; 3];
                for i in 0..6 {
                    for d in 0..3 {
                        t_xi[d] += grads[i][0] * phys_nodes[i][d];
                        t_eta[d] += grads[i][1] * phys_nodes[i][d];
                    }
                }
                let area_elem = norm3(cross(t_xi, t_eta));
                for i in 0..6 {
                    nodal_weights[i] += shapes[i] * area_elem * qp.weight;
                }
            }
        }
    }

    // Scatter into global f.
    for (i, &global_node) in connectivity.iter().enumerate() {
        for alpha in 0..3 {
            f[3 * global_node + alpha] += nodal_weights[i] * traction[alpha];
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::assembly::ElementOrder;
    use crate::assembly::test_support::scaled_p2_phys_nodes;

    const TOL: f64 = 1e-12;

    // =======================================================================
    // apply_point_load
    // =======================================================================

    /// `apply_point_load` places force at the correct global DOFs and leaves
    /// all other entries unchanged.
    #[test]
    fn apply_point_load_adds_force_at_correct_global_dofs() {
        let mut f = vec![0.0_f64; 15]; // 5 nodes × 3 DOFs
        apply_point_load(&mut f, 2, [10.0, -5.0, 7.0]);
        // DOF formula: 3*node + α
        assert_eq!(f[6], 10.0, "f[3*2+0] should be 10.0");
        assert_eq!(f[7], -5.0, "f[3*2+1] should be -5.0");
        assert_eq!(f[8], 7.0, "f[3*2+2] should be 7.0");
        // All other entries must remain 0.0.
        for i in 0..15 {
            if i != 6 && i != 7 && i != 8 {
                assert_eq!(f[i], 0.0, "f[{i}] should remain 0.0");
            }
        }
    }

    /// Second call accumulates rather than overwrites (`+=` semantics).
    #[test]
    fn apply_point_load_accumulates_on_second_call() {
        let mut f = vec![0.0_f64; 15];
        apply_point_load(&mut f, 2, [10.0, -5.0, 7.0]);
        apply_point_load(&mut f, 2, [3.0, 1.0, -2.0]);
        assert_eq!(f[6], 13.0, "f[6] should be 10+3=13");
        assert_eq!(f[7], -4.0, "f[7] should be -5+1=-4");
        assert_eq!(f[8], 5.0, "f[8] should be 7+(-2)=5");
    }

    /// Out-of-range node panics with a message naming the node index.
    #[test]
    #[should_panic(expected = "node 5")]
    fn apply_point_load_panics_on_out_of_range_node() {
        let mut f = vec![0.0_f64; 15]; // 5 nodes, valid nodes 0..4
        apply_point_load(&mut f, 5, [1.0, 0.0, 0.0]); // node 5 → 3*5+2=17 >= 15
    }

    /// Non-multiple-of-3 f.len() panics.
    #[test]
    #[should_panic(expected = "f.len() = 14")]
    fn apply_point_load_panics_on_non_multiple_of_3_f_len() {
        let mut f = vec![0.0_f64; 14];
        apply_point_load(&mut f, 0, [1.0, 0.0, 0.0]);
    }

    /// Empty f (len=0) panics even for node 0.
    #[test]
    #[should_panic]
    fn apply_point_load_panics_on_empty_f() {
        let mut f: Vec<f64> = vec![];
        apply_point_load(&mut f, 0, [1.0, 0.0, 0.0]);
    }

    // =======================================================================
    // apply_body_force — P1
    // =======================================================================

    /// Unit body force on the P1 reference tet. Each of the 4 nodes receives
    /// `vol / 4 = (1/6) / 4 = 1/24` weight per unit body force.
    #[test]
    fn apply_body_force_p1_reference_tet_correct_per_node_weight() {
        let phys_nodes: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let connectivity = [0usize, 1, 2, 3];
        let mut f = vec![0.0_f64; 12]; // 4 nodes × 3 DOFs
        apply_body_force(&mut f, ElementOrder::P1, &connectivity, &phys_nodes, [1.0, 2.0, 3.0]);

        let expected_weight = 1.0 / 24.0; // vol/4 = (1/6)/4
        let force = [1.0, 2.0, 3.0];
        for node in 0..4 {
            for alpha in 0..3 {
                let got = f[3 * node + alpha];
                let expected = expected_weight * force[alpha];
                assert!(
                    (got - expected).abs() < TOL,
                    "node {node} axis {alpha}: got {got}, expected {expected}",
                );
            }
        }
    }

    // =======================================================================
    // apply_body_force — P2
    // =======================================================================

    /// P2 reference tet body force: vertex nodes get -vol/120 = -1/720,
    /// edge-midpoint nodes get vol/30 = 1/180.
    ///
    /// Standard analytical result for P2 constant body-force lumping.
    #[test]
    fn apply_body_force_p2_reference_tet_vertex_and_midpoint_weights() {
        let phys = scaled_p2_phys_nodes(1.0);
        let connectivity: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut f = vec![0.0_f64; 30]; // 10 nodes × 3 DOFs
        apply_body_force(&mut f, ElementOrder::P2, &connectivity, &phys, [1.0, 0.0, 0.0]);

        let vol = 1.0 / 6.0;
        let expected_vertex = -vol / 20.0;  // -1/120 * vol = -vol/120 — standard P2 lumping
        let expected_midpt = vol / 5.0;     // actual P2 edge-midpoint weight (4*vol/20 = vol/5)

        // Let me recalculate this analytically.
        // For a P2 tet with constant body force ρ·b, the consistent body force is:
        //   f_i = ∫ N_i dV · b
        //
        // For vertex node i, N_i = λ_i(2λ_i - 1)
        //   ∫_T λ_i(2λ_i - 1) dV = ∫_T (2λ_i² - λ_i) dV
        //   = 2·∫ λ_i² dV - ∫ λ_i dV
        //   = 2 · vol/(5!/(2!·2!)) ...
        // Standard integrals over unit tet: ∫_T λ_0^a λ_1^b λ_2^c λ_3^d dV = a!b!c!d!/(a+b+c+d+3)! · vol_factor
        // For unit tet (vol=1/6): ∫ λ_i dV = 1/4 · 1/6 = 1/24
        // ∫ λ_i² dV = 2! · 1!⁰ / (2+3)! · (3!) ...
        // Actually: ∫_T λ_i^2 dV = 2·1·1·1/(2+1+1+1)! · (1·1·1·1) ...
        // The standard formula: ∫_T λ_0^a λ_1^b λ_2^c λ_3^d dV = a!b!c!d!/(a+b+c+d+3)! · 6·vol
        // where vol = 1/6 for the reference tet, so 6·vol = 1.
        //   ∫ λ_i dV = 1!·0!·0!·0!/(1+3)! · 1 = 1/24
        //   ∫ λ_i² dV = 2!·0!·0!·0!/(2+3)! · 1 = 2/120 = 1/60
        //   So ∫ N_vertex dV = 2·(1/60) - 1/24 = 1/30 - 1/24 = (4-5)/120 = -1/120
        //
        // For edge-midpoint node (a,b): N = 4λ_a·λ_b
        //   ∫ 4λ_a·λ_b dV = 4 · 1!·1!·0!·0!/(1+1+3)! · 1 = 4/(5!) = 4/120 = 1/30
        //
        // So vertex weight = -1/120, edge-midpoint weight = 1/30 = 4/120.
        // These are per unit body-force, NOT multiplied by vol separately because
        // these integrals already account for the physical-space volume.

        // For unit reference tet (vol = 1/6):
        // vertex nodes: f_i = -1/120
        // edge-midpoint nodes: f_i = 1/30

        let expected_vertex = -1.0 / 120.0;
        let expected_midpt = 1.0 / 30.0;

        // Verify total force conservation: Σ f_i = vol = 1/6
        let total_x: f64 = (0..10).map(|i| f[3 * i]).sum();
        assert!(
            (total_x - 1.0 / 6.0).abs() < TOL,
            "total x-force = {total_x}, expected vol = 1/6",
        );

        // Vertex nodes (0..3).
        for node in 0..4 {
            let got = f[3 * node];
            assert!(
                (got - expected_vertex).abs() < TOL,
                "vertex node {node}: got {got}, expected {expected_vertex}",
            );
        }

        // Edge-midpoint nodes (4..10).
        for node in 4..10 {
            let got = f[3 * node];
            assert!(
                (got - expected_midpt).abs() < TOL,
                "midpoint node {node}: got {got}, expected {expected_midpt}",
            );
        }
    }

    // =======================================================================
    // apply_body_force — volume scaling
    // =======================================================================

    /// Volume scales as s³ for uniform 3D scaling. Per-node body-force weight
    /// = (s³/6) / 4 for a P1 tet scaled by s in each axis.
    #[test]
    fn apply_body_force_p1_volume_scaling() {
        for &s in &[0.5_f64, 1.0, 2.0, 3.7] {
            let phys: [[f64; 3]; 4] = [
                [0.0, 0.0, 0.0],
                [s, 0.0, 0.0],
                [0.0, s, 0.0],
                [0.0, 0.0, s],
            ];
            let connectivity = [0usize, 1, 2, 3];
            let mut f = vec![0.0_f64; 12];
            apply_body_force(&mut f, ElementOrder::P1, &connectivity, &phys, [1.0, 0.0, 0.0]);

            let vol = s * s * s / 6.0;
            let expected_per_node = vol / 4.0;
            for node in 0..4 {
                let got = f[3 * node]; // x-axis only (force=[1,0,0])
                assert!(
                    (got - expected_per_node).abs() < TOL * s * s * s,
                    "scale={s}: node {node} x-DOF = {got}, expected {expected_per_node}",
                );
                // y and z DOFs must be zero.
                assert_eq!(f[3 * node + 1], 0.0);
                assert_eq!(f[3 * node + 2], 0.0);
            }
        }
    }

    // =======================================================================
    // apply_body_force — contract panics
    // =======================================================================

    #[test]
    #[should_panic(expected = "connectivity.len()")]
    fn apply_body_force_p1_wrong_connectivity_len() {
        let phys: [[f64; 3]; 4] = [[0.0; 3]; 4];
        let conn = [0usize, 1, 2]; // 3 instead of 4
        let mut f = vec![0.0_f64; 12];
        apply_body_force(&mut f, ElementOrder::P1, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "phys_nodes.len()")]
    fn apply_body_force_p2_wrong_phys_nodes_len() {
        let phys: [[f64; 3]; 8] = [[0.0; 3]; 8]; // 8 instead of 10
        let conn: Vec<usize> = (0..10).collect();
        let mut f = vec![0.0_f64; 30];
        apply_body_force(&mut f, ElementOrder::P2, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "f.len() = 11")]
    fn apply_body_force_f_len_not_multiple_of_3() {
        let phys: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let conn = [0usize, 1, 2, 3];
        let mut f = vec![0.0_f64; 11];
        apply_body_force(&mut f, ElementOrder::P1, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn apply_body_force_connectivity_out_of_range() {
        let phys: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let conn = [0usize, 1, 2, 99]; // 99 out of range for f.len()/3 = 4
        let mut f = vec![0.0_f64; 12];
        apply_body_force(&mut f, ElementOrder::P1, &conn, &phys, [0.0; 3]);
    }

    // =======================================================================
    // apply_traction_load — P1Tri
    // =======================================================================

    /// Unit right triangle in xy-plane, area = 1/2. Each of 3 nodes receives
    /// area/3 = 1/6 of the traction force.
    #[test]
    fn apply_traction_p1tri_unit_right_triangle() {
        let face_phys: [[f64; 3]; 3] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let conn = [0usize, 1, 2];
        let mut f = vec![0.0_f64; 15]; // 5 nodes
        apply_traction_load(&mut f, FaceOrder::P1Tri, &conn, &face_phys, [1.0, 2.0, 3.0]);

        let expected_per_node = 1.0 / 6.0; // area/3 = (1/2)/3
        let traction = [1.0, 2.0, 3.0];
        for node in 0..3 {
            for alpha in 0..3 {
                let got = f[3 * node + alpha];
                let expected = expected_per_node * traction[alpha];
                assert!(
                    (got - expected).abs() < TOL,
                    "node {node} axis {alpha}: got {got}, expected {expected}",
                );
            }
        }

        // Conservation: sum of nodal forces == area * traction.
        let area = 0.5;
        for alpha in 0..3 {
            let total: f64 = (0..3).map(|n| f[3 * n + alpha]).sum();
            assert!(
                (total - area * traction[alpha]).abs() < TOL,
                "axis {alpha}: total = {total}, expected {} * {} = {}",
                area, traction[alpha], area * traction[alpha],
            );
        }

        // Nodes 3 and 4 must be untouched.
        for i in 9..15 {
            assert_eq!(f[i], 0.0, "f[{i}] should be 0.0 (not a face node)");
        }
    }

    // =======================================================================
    // apply_traction_load — P2Tri
    // =======================================================================

    /// Straight-edge unit right triangle (area=1/2) with quadratic face nodes.
    /// For constant traction on a P2 triangle: vertex nodes get 0, edge-midpoint
    /// nodes each get area/3 = 1/6.
    ///
    /// This is the standard analytical result for P2 constant-traction lumping:
    /// ∫ N_vertex dA = 0, ∫ N_midpoint dA = 1/6 (for unit-right-triangle area 1/2).
    #[test]
    fn apply_traction_p2tri_unit_right_triangle() {
        let face_phys: [[f64; 3]; 6] = [
            [0.0, 0.0, 0.0], // v0
            [1.0, 0.0, 0.0], // v1
            [0.0, 1.0, 0.0], // v2
            [0.5, 0.0, 0.0], // m01
            [0.5, 0.5, 0.0], // m12
            [0.0, 0.5, 0.0], // m20
        ];
        let conn: [usize; 6] = [0, 1, 2, 3, 4, 5];
        let mut f = vec![0.0_f64; 30]; // 10 nodes
        apply_traction_load(&mut f, FaceOrder::P2Tri, &conn, &face_phys, [1.0, 0.0, 0.0]);

        // Vertex nodes (0..3) get 0.
        for node in 0..3 {
            assert!(
                f[3 * node].abs() < TOL,
                "vertex node {node} x-DOF = {}, expected 0",
                f[3 * node],
            );
        }

        // Edge-midpoint nodes (3..6) each get area/3 = 1/6.
        let expected_midpt = 1.0 / 6.0;
        for node in 3..6 {
            let got = f[3 * node];
            assert!(
                (got - expected_midpt).abs() < TOL,
                "midpoint node {node} x-DOF = {got}, expected {expected_midpt}",
            );
        }

        // Conservation: sum = area * traction_x = 0.5 * 1.0 = 0.5.
        let total_x: f64 = (0..6).map(|n| f[3 * n]).sum();
        assert!(
            (total_x - 0.5).abs() < TOL,
            "total x-force = {total_x}, expected 0.5",
        );
    }

    // =======================================================================
    // apply_traction_load — contract panics
    // =======================================================================

    #[test]
    #[should_panic(expected = "connectivity.len()")]
    fn apply_traction_p1tri_wrong_connectivity_len() {
        let phys: [[f64; 3]; 3] = [[0.0; 3]; 3];
        let conn = [0usize, 1]; // 2 instead of 3
        let mut f = vec![0.0_f64; 9];
        apply_traction_load(&mut f, FaceOrder::P1Tri, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "phys_nodes.len()")]
    fn apply_traction_p2tri_wrong_phys_nodes_len() {
        let phys: [[f64; 3]; 3] = [[0.0; 3]; 3]; // 3 instead of 6
        let conn = [0usize, 1, 2, 3, 4, 5];
        let mut f = vec![0.0_f64; 21];
        apply_traction_load(&mut f, FaceOrder::P2Tri, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "f.len() = 7")]
    fn apply_traction_f_len_not_multiple_of_3() {
        let phys: [[f64; 3]; 3] = [[0.0; 3]; 3];
        let conn = [0usize, 1, 2];
        let mut f = vec![0.0_f64; 7];
        apply_traction_load(&mut f, FaceOrder::P1Tri, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn apply_traction_connectivity_out_of_range() {
        let phys: [[f64; 3]; 3] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let conn = [0usize, 1, 99]; // 99 out of range for f.len()/3 = 3
        let mut f = vec![0.0_f64; 9];
        apply_traction_load(&mut f, FaceOrder::P1Tri, &conn, &phys, [0.0; 3]);
    }

    // =======================================================================
    // Additive accumulation: all three primitives compose into shared f
    // =======================================================================

    /// Starting from a zero load vector, apply traction + body force + point
    /// load. The combined result equals the sum of the individual contributions.
    ///
    /// Pins the additive-accumulation (`+=`) contract across all three
    /// primitives: a regression that makes any one primitive overwrite instead
    /// of accumulate would surface here.
    #[test]
    fn all_three_primitives_accumulate_additively() {
        let tet_phys: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let tet_conn = [0usize, 1, 2, 3];

        // Bottom face (nodes 0,1,2) of the unit tet.
        let face_phys: [[f64; 3]; 3] = [tet_phys[0], tet_phys[1], tet_phys[2]];
        let face_conn = [0usize, 1, 2];

        // Reference individual contributions on fresh zero vectors.
        let mut f_traction = vec![0.0_f64; 12];
        apply_traction_load(
            &mut f_traction,
            FaceOrder::P1Tri,
            &face_conn,
            &face_phys,
            [10.0, 0.0, 0.0],
        );

        let mut f_body = vec![0.0_f64; 12];
        apply_body_force(
            &mut f_body,
            ElementOrder::P1,
            &tet_conn,
            &tet_phys,
            [0.0, -9.81, 0.0],
        );

        let mut f_point = vec![0.0_f64; 12];
        apply_point_load(&mut f_point, 2, [100.0, 0.0, 0.0]);

        // Combined: apply all three to the same shared vector.
        let mut f_combined = vec![0.0_f64; 12];
        apply_traction_load(
            &mut f_combined,
            FaceOrder::P1Tri,
            &face_conn,
            &face_phys,
            [10.0, 0.0, 0.0],
        );
        apply_body_force(
            &mut f_combined,
            ElementOrder::P1,
            &tet_conn,
            &tet_phys,
            [0.0, -9.81, 0.0],
        );
        apply_point_load(&mut f_combined, 2, [100.0, 0.0, 0.0]);

        // Assert bit-for-bit equality (each contribution lands on a fresh entry
        // path in this fixture, so IEEE 754 makes bit-equality achievable).
        for i in 0..12 {
            let expected = f_traction[i] + f_body[i] + f_point[i];
            assert_eq!(
                f_combined[i].to_bits(),
                expected.to_bits(),
                "f_combined[{i}] = {} ≠ f_traction[{i}] + f_body[{i}] + f_point[{i}] = {}",
                f_combined[i],
                expected,
            );
        }
    }
}
