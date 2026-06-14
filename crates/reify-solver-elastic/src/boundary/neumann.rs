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

/// Face interpolation order.
///
/// Distinguishes 3-node linear triangular faces (`P1Tri`), 6-node quadratic
/// triangular faces (`P2Tri`), and 4-node bilinear quadrilateral faces
/// (`P1Quad`). Separate from [`crate::assembly::ElementOrder`] (which keys on
/// volume-element node count) because surface tractions key on face node
/// count — reusing `ElementOrder` would invite confusion at call sites.
///
/// Mapping to volume element kinds:
///
/// - Tet faces ⇒ `P1Tri` (4-node tet) or `P2Tri` (10-node tet).
/// - Hex faces ⇒ `P1Quad` (all 6 faces of an 8-node hex are bilinear quads).
/// - Wedge faces ⇒ `P1Tri` (the two triangle end-caps) or `P1Quad` (the three
///   bilinear quad side faces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceOrder {
    /// 3-node linear triangular face (P1 tet face).
    P1Tri,
    /// 6-node quadratic triangular face (P2 tet face): 3 vertices followed by
    /// 3 edge-midpoints in canonical order `[v0, v1, v2, m_{01}, m_{12}, m_{20}]`.
    P2Tri,
    /// 4-node bilinear quadrilateral face used by hex elements and the side
    /// faces of wedge elements.
    ///
    /// Node ordering: canonical Hughes/Gmsh hex8 bottom-face order — vertices
    /// at reference coords `(-1, -1)`, `(+1, -1)`, `(+1, +1)`, `(-1, +1)`
    /// traversed counter-clockwise when viewed from the outward face normal.
    /// This matches the four "ζ = −1" nodes of a hex
    /// (`crate::elements::hex_p1::VERTEX_SIGNS[0..4]`) so callers extracting a
    /// hex's bottom-face nodes can pass `&hex_phys[0..4]` directly.
    P1Quad,
}

/// Maximum `E::N_NODES` across element types dispatched by [`apply_body_force`].
///
/// Currently `max(TetP1::N_NODES = 4, TetP2::N_NODES = 10) = 10`. Used to
/// stack-allocate the per-call `nodal_weights` buffer in
/// [`integrate_body_force_generic`], avoiding heap traffic on the FEA
/// load-assembly hot path. Bump this value when adding a higher-order
/// element (e.g. P3 tet, 20 nodes) to the [`apply_body_force`] dispatch.
const MAX_BODY_FORCE_NODES: usize = 10;

// The compile-time guard for `MAX_BODY_FORCE_NODES` lives inside
// `integrate_body_force_generic` as an inline `const { assert!(...) }` block
// (see below). Because `const { ... }` is evaluated per monomorphization, any
// element type routed through that function is checked automatically — without
// requiring a separate top-level assertion for each dispatch arm.

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
    // Structural compile-time guard: evaluated once per monomorphization, so
    // any element type routed through `apply_body_force` (or any future caller)
    // is automatically checked — no per-dispatch-arm top-level assertion needed.
    // If the assert fires, bump `MAX_BODY_FORCE_NODES` to fit the new element
    // type's node count.
    const {
        assert!(
            E::N_NODES <= MAX_BODY_FORCE_NODES,
            "E::N_NODES exceeds MAX_BODY_FORCE_NODES; bump the constant to fit the new element type's node count"
        )
    };

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

    // Stack-allocate the per-element nodal-weight accumulator to avoid
    // per-call heap traffic on the FEA load-assembly hot path. Sized for
    // the largest element currently dispatched (TetP2::N_NODES = 10);
    // sliced to the actual count for this element type. The `const { assert! }`
    // block above ensures at compile time that E::N_NODES fits within the
    // buffer, so this slice is always in-bounds.
    let mut nodal_weights_buf = [0.0_f64; MAX_BODY_FORCE_NODES];
    let nodal_weights = &mut nodal_weights_buf[..E::N_NODES];
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

/// A 2D reference coordinate `(ξ, η)` on a face element.
///
/// Shared between triangle faces (P1Tri/P2Tri, reference triangle vertices
/// `(0,0), (1,0), (0,1)`) and quadrilateral faces (P1Quad, reference square
/// vertices `(±1, ±1)`). The triangle and quad reference geometries differ in
/// their valid coordinate domain, but the `(ξ, η)` storage is identical and
/// `integrate_face_generic` is parametric over both via this single type plus
/// caller-supplied shape / gradient / quadrature-rule values.
#[derive(Clone, Copy)]
struct FaceRefCoord {
    xi: f64,
    eta: f64,
}

/// A quadrature point on a face element.
///
/// See [`FaceRefCoord`] — shared between triangle and quad face quadrature
/// rules.
#[derive(Clone, Copy)]
struct FaceQuadPoint {
    coord: FaceRefCoord,
    weight: f64,
}

/// 1-point centroid rule for the unit reference triangle (degree-1 exact).
///
/// Point at `(1/3, 1/3)`, weight `1/2` (= reference-triangle area).
const TRI_P1_QUADRATURE: &[FaceQuadPoint] = &[FaceQuadPoint {
    coord: FaceRefCoord {
        xi: 1.0 / 3.0,
        eta: 1.0 / 3.0,
    },
    weight: 0.5,
}];

/// 3-point edge-midpoint rule for the unit reference triangle (degree-2 exact).
///
/// Points at the midpoints of the three edges: `(1/2, 0)`, `(1/2, 1/2)`,
/// `(0, 1/2)`, each with weight `1/6`. Total weight `1/2` = triangle area.
const TRI_P2_QUADRATURE: &[FaceQuadPoint] = &[
    FaceQuadPoint {
        coord: FaceRefCoord { xi: 0.5, eta: 0.0 },
        weight: 1.0 / 6.0,
    },
    FaceQuadPoint {
        coord: FaceRefCoord { xi: 0.5, eta: 0.5 },
        weight: 1.0 / 6.0,
    },
    FaceQuadPoint {
        coord: FaceRefCoord { xi: 0.0, eta: 0.5 },
        weight: 1.0 / 6.0,
    },
];

/// P1 triangle shape functions `[N_0, N_1, N_2]` at a reference coordinate.
///
/// `N_0 = 1 - ξ - η`, `N_1 = ξ`, `N_2 = η`.
fn tri_p1_shape(c: FaceRefCoord) -> [f64; 3] {
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
/// Follows the standard 6-node quadratic Lagrange triangle ordering.
fn tri_p2_shape(c: FaceRefCoord) -> [f64; 6] {
    let xi = c.xi;
    let eta = c.eta;
    let l0 = 1.0 - xi - eta;
    let l1 = xi;
    let l2 = eta;
    [
        l0 * (2.0 * l0 - 1.0), // N_0
        l1 * (2.0 * l1 - 1.0), // N_1
        l2 * (2.0 * l2 - 1.0), // N_2
        4.0 * l0 * l1,         // N_3 (edge 01 midpoint)
        4.0 * l1 * l2,         // N_4 (edge 12 midpoint)
        4.0 * l0 * l2,         // N_5 (edge 20 midpoint)
    ]
}

/// P2 triangle shape-function gradients `[∂N_i/∂ξ, ∂N_i/∂η]`.
fn tri_p2_grads(c: FaceRefCoord) -> [[f64; 2]; 6] {
    let xi = c.xi;
    let eta = c.eta;
    let l0 = 1.0 - xi - eta;
    [
        // ∂l0/∂ξ = ∂l0/∂η = -1, so both partials of N_0 share the same expression.
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

// ---------------------------------------------------------------------------
// Quad face reference geometry, quadrature, shape functions, and gradients.
//
// Reference geometry (`(±1, ±1)`, area 4) and quadrature rule live here;
// the coordinate/quadrature-point types ([`FaceRefCoord`] / [`FaceQuadPoint`])
// are shared with the triangle path so a single [`integrate_face_generic`]
// drives both face shapes.
// ---------------------------------------------------------------------------

/// 1D 2-point Gauss-Legendre point on `[-1, +1]`: `1/√3`.
///
/// Same literal value as [`crate::elements::hex_p1::HEX_P1_GAUSS_PT`] —
/// kept bit-identical so the quad-face surface integral and the hex
/// volume integral share the same numeric building block.
const QUAD_P1_GAUSS_PT: f64 = 0.5773502691896257;

/// 2×2 Gauss-Legendre quadrature rule for the reference quad `[-1, 1]²`
/// (degree-3-per-axis exact). Four points at `(±1/√3, ±1/√3)`, each with
/// weight `1`; total weight `4` = reference-quad area.
///
/// Naming: `QUAD_P1_QUADRATURE` (not `QUAD_P1_QUAD`) so "quad" appears once
/// — the `QUAD_` prefix already disambiguates the face shape, and the
/// `_QUADRATURE` suffix names the rule itself; the previous spelling
/// `QUAD_P1_QUAD` overloaded "quad" with two meanings (quadrilateral and
/// quadrature) at the call site.
const QUAD_P1_QUADRATURE: &[FaceQuadPoint] = &[
    FaceQuadPoint {
        coord: FaceRefCoord {
            xi: -QUAD_P1_GAUSS_PT,
            eta: -QUAD_P1_GAUSS_PT,
        },
        weight: 1.0,
    },
    FaceQuadPoint {
        coord: FaceRefCoord {
            xi: QUAD_P1_GAUSS_PT,
            eta: -QUAD_P1_GAUSS_PT,
        },
        weight: 1.0,
    },
    FaceQuadPoint {
        coord: FaceRefCoord {
            xi: QUAD_P1_GAUSS_PT,
            eta: QUAD_P1_GAUSS_PT,
        },
        weight: 1.0,
    },
    FaceQuadPoint {
        coord: FaceRefCoord {
            xi: -QUAD_P1_GAUSS_PT,
            eta: QUAD_P1_GAUSS_PT,
        },
        weight: 1.0,
    },
];

/// P1 bilinear quad shape functions `[N_0, N_1, N_2, N_3]` at a reference
/// coordinate.
///
/// Canonical Hughes/Gmsh hex8 bottom-face ordering — node `i` is at vertex
/// `(±1, ±1)` with the sign pattern `(-1,-1), (+1,-1), (+1,+1), (-1,+1)`
/// traversed counter-clockwise.
///
/// `N_0 = (1-ξ)(1-η)/4`, `N_1 = (1+ξ)(1-η)/4`,
/// `N_2 = (1+ξ)(1+η)/4`, `N_3 = (1-ξ)(1+η)/4`.
fn quad_p1_shape(c: FaceRefCoord) -> [f64; 4] {
    let xi = c.xi;
    let eta = c.eta;
    [
        0.25 * (1.0 - xi) * (1.0 - eta),
        0.25 * (1.0 + xi) * (1.0 - eta),
        0.25 * (1.0 + xi) * (1.0 + eta),
        0.25 * (1.0 - xi) * (1.0 + eta),
    ]
}

/// P1 bilinear quad shape-function gradients `[∂N_i/∂ξ, ∂N_i/∂η]` per node.
fn quad_p1_grads(c: FaceRefCoord) -> [[f64; 2]; 4] {
    let xi = c.xi;
    let eta = c.eta;
    [
        // N_0 = (1-ξ)(1-η)/4
        [-0.25 * (1.0 - eta), -0.25 * (1.0 - xi)],
        // N_1 = (1+ξ)(1-η)/4
        [0.25 * (1.0 - eta), -0.25 * (1.0 + xi)],
        // N_2 = (1+ξ)(1+η)/4
        [0.25 * (1.0 + eta), 0.25 * (1.0 + xi)],
        // N_3 = (1-ξ)(1+η)/4
        [-0.25 * (1.0 + eta), 0.25 * (1.0 - xi)],
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

/// Generic surface-traction integrator over a single face element.
///
/// Computes `w_i = Σ_q N_i(q.coord) · |t_ξ × t_η|(q) · q.weight` for each
/// face node `i` using the caller-supplied quadrature rule `quad` and shape /
/// gradient functions `shape_fn` / `grad_fn`, then scatters
/// `f[3 * connectivity[i] + α] += w_i * traction[α]`.
///
/// Drives all face shapes — the triangle path (`P1Tri`/`P2Tri`) and the
/// quadrilateral path (`P1Quad`) both reduce to this single helper. The area-
/// element `|t_ξ × t_η|` math and scatter step are identical regardless of
/// the reference shape, since both triangle and quad reference geometries
/// produce a 3D physical surface from a 2D `(ξ, η)` reference parameter
/// space. The shape/grad/quad inputs encode the per-face-shape geometry and
/// quadrature; the integrator itself is shape-agnostic.
///
/// # Panics (unconditional, Task-2544 contract-explicitness convention)
///
/// - `connectivity.len() != N`
/// - `phys_nodes.len() != N`
/// - `f.len() % 3 != 0`
/// - Any entry in `connectivity` is `>= f.len() / 3` (out-of-range global node)
fn integrate_face_generic<const N: usize>(
    f: &mut [f64],
    connectivity: &[usize],
    phys_nodes: &[[f64; 3]],
    traction: [f64; 3],
    quad: &[FaceQuadPoint],
    shape_fn: impl Fn(FaceRefCoord) -> [f64; N],
    grad_fn: impl Fn(FaceRefCoord) -> [[f64; 2]; N],
) {
    assert_eq!(
        connectivity.len(),
        N,
        "integrate_face_generic: connectivity.len() = {} but expected {} face nodes",
        connectivity.len(),
        N,
    );
    assert_eq!(
        phys_nodes.len(),
        N,
        "integrate_face_generic: phys_nodes.len() = {} but expected {} face nodes",
        phys_nodes.len(),
        N,
    );
    assert!(
        f.len().is_multiple_of(3),
        "integrate_face_generic: f.len() = {} is not a multiple of 3; \
         the global load vector must have exactly 3 DOFs per node",
        f.len(),
    );
    let n_global_nodes = f.len() / 3;
    for (local_i, &global_node) in connectivity.iter().enumerate() {
        assert!(
            global_node < n_global_nodes,
            "integrate_face_generic: connectivity[{}] = {} is out of range; \
             f.len() / 3 = {} global nodes",
            local_i,
            global_node,
            n_global_nodes,
        );
    }

    // Accumulate per-node integration weights w_i = Σ_q N_i(q) · |t_ξ × t_η|(q) · q.weight.
    let mut nodal_weights = [0.0_f64; N];
    for qp in quad {
        let shapes = shape_fn(qp.coord);
        let grads = grad_fn(qp.coord);
        // Tangent vectors: t_ξ = Σ_i (∂N_i/∂ξ) · phys_nodes[i]
        let mut t_xi = [0.0_f64; 3];
        let mut t_eta = [0.0_f64; 3];
        for i in 0..N {
            for d in 0..3 {
                t_xi[d] += grads[i][0] * phys_nodes[i][d];
                t_eta[d] += grads[i][1] * phys_nodes[i][d];
            }
        }
        let area_elem = norm3(cross(t_xi, t_eta));
        for i in 0..N {
            nodal_weights[i] += shapes[i] * area_elem * qp.weight;
        }
    }

    // Scatter into global f.
    for (i, &global_node) in connectivity.iter().enumerate() {
        for alpha in 0..3 {
            f[3 * global_node + alpha] += nodal_weights[i] * traction[alpha];
        }
    }
}

/// Apply a uniform surface traction over a single face.
///
/// Computes `∫_Γ N^T t dA` via face quadrature, using the surface area
/// element `|∂x/∂ξ × ∂x/∂η|` built from the face's physical-node coordinates.
/// Accumulates the result into `f`.
///
/// - `FaceOrder::P1Tri` — 3-node linear triangle, 1-point centroid quadrature
///   (degree-1 exact for the `N · const` integrand).
/// - `FaceOrder::P2Tri` — 6-node quadratic triangle in canonical order
///   `[v0, v1, v2, m_{01}, m_{12}, m_{20}]`, 3-point edge-midpoint quadrature
///   (degree-2 exact for straight-edged P2 triangles where edge midpoints lie
///   at the geometric midpoint and the surface map is affine). For genuinely
///   curved P2 faces the Jacobian `|t_ξ × t_η|` is non-polynomial and the
///   3-point rule is approximate; this situation does not arise in the standard
///   FEA practice of straight-edged tetrahedral meshes.
/// - `FaceOrder::P1Quad` — 4-node bilinear quadrilateral (hex face or wedge
///   side face) in canonical Hughes/Gmsh hex8 bottom-face order, 2×2
///   Gauss-Legendre quadrature on the reference cube face `[-1, 1]²`
///   (degree-3-per-axis exact for the bilinear-shape × constant-Jacobian
///   integrand).
///
/// # Additive semantics
///
/// Uses `+=`; multiple traction calls accumulate correctly. See module doc.
///
/// # Panics
///
/// - `connectivity.len()` does not match the face node count (3 for P1Tri,
///   6 for P2Tri, 4 for P1Quad).
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
    match face_order {
        FaceOrder::P1Tri => integrate_face_generic(
            f,
            connectivity,
            phys_nodes,
            traction,
            TRI_P1_QUADRATURE,
            tri_p1_shape,
            |_| TRI_P1_GRADS,
        ),
        FaceOrder::P2Tri => integrate_face_generic(
            f,
            connectivity,
            phys_nodes,
            traction,
            TRI_P2_QUADRATURE,
            tri_p2_shape,
            tri_p2_grads,
        ),
        FaceOrder::P1Quad => integrate_face_generic(
            f,
            connectivity,
            phys_nodes,
            traction,
            QUAD_P1_QUADRATURE,
            quad_p1_shape,
            quad_p1_grads,
        ),
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
        apply_body_force(
            &mut f,
            ElementOrder::P1,
            &connectivity,
            &phys_nodes,
            [1.0, 2.0, 3.0],
        );

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

    /// Kernel→result half of the task-4440 β §7.1 boundary test (reciprocal
    /// characterisation of the gravity bridge).
    ///
    /// For the unit reference tet (volume = 1/6) with body force [0, 0, -ρg]:
    ///   (i)  every node's x- and y-DOF force is exactly 0  (only z component)
    ///   (ii) every node's z-DOF force < 0                  (purely downward)
    ///   (iii) Σ_i f_z[i] = body_force_z × Volume exactly  (partition of unity)
    ///
    /// Partition-of-unity identity: Σ_i ∫ N_i dV = V  →  total scattered force
    /// equals body_force_z · Volume, independent of mesh density.
    ///
    /// This pins the total-weight + sign invariant the gravity bridge relies on;
    /// combined with the eval-layer step-1 test, a units bug on either side
    /// fails one direction.  The kernel is pre-existing and correct, so this
    /// test passes on write.
    #[test]
    fn apply_body_force_p1_gravity_vector_downward_total_weight() {
        let phys_nodes: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let connectivity = [0usize, 1, 2, 3];
        let body_force_z = -76982.2_f64; // representative ρg  (N·m⁻³)
        let body_force = [0.0_f64, 0.0, body_force_z];
        let volume = 1.0_f64 / 6.0; // unit reference tet volume

        let mut f = vec![0.0_f64; 12]; // 4 nodes × 3 DOFs
        apply_body_force(&mut f, ElementOrder::P1, &connectivity, &phys_nodes, body_force);

        for node in 0..4 {
            // (i) x- and y-DOFs must be zero — body force has no x/y component
            assert_eq!(
                f[3 * node],
                0.0,
                "node {node} x-DOF: expected 0.0, got {}",
                f[3 * node]
            );
            assert_eq!(
                f[3 * node + 1],
                0.0,
                "node {node} y-DOF: expected 0.0, got {}",
                f[3 * node + 1]
            );
            // (ii) z-DOF must be strictly negative (downward body force)
            let fz = f[3 * node + 2];
            assert!(
                fz < 0.0,
                "node {node} z-DOF: expected < 0.0 (downward), got {fz}"
            );
        }

        // (iii) partition-of-unity: Σ f_z[i] = body_force_z × volume  (exact)
        let z_sum: f64 = (0..4).map(|n| f[3 * n + 2]).sum();
        let expected_z_sum = body_force_z * volume;
        assert!(
            (z_sum - expected_z_sum).abs() < 1e-9,
            "z-force sum: expected {expected_z_sum:.9}, got {z_sum:.9} (diff {})",
            (z_sum - expected_z_sum).abs()
        );
    }

    /// Pins the scatter target: non-contiguous connectivity `[4, 0, 7, 2]` into
    /// a 10-node global `f` (length 30). A regression treating local index `i`
    /// as the global index would write into `f[0..12]` instead of the correct
    /// `{f[12..15], f[0..3], f[21..24], f[6..9]}` and fail this test.
    #[test]
    fn apply_body_force_p1_non_contiguous_connectivity_scatter() {
        let phys_nodes: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        // Local nodes 0/1/2/3 → global nodes 4/0/7/2.
        let conn = [4usize, 0, 7, 2];
        let mut f = vec![0.0_f64; 30]; // 10 global nodes × 3 DOFs
        apply_body_force(
            &mut f,
            ElementOrder::P1,
            &conn,
            &phys_nodes,
            [1.0, 0.0, 0.0],
        );

        // vol/4 = (1/6)/4 = 1/24 per local node on the unit reference tet.
        let expected = 1.0 / 24.0;

        // Global node 4: f[12..15]
        assert!(
            (f[12] - expected).abs() < TOL,
            "f[12] (node 4 x-DOF) = {}, expected {expected}",
            f[12]
        );
        assert_eq!(f[13], 0.0, "f[13] (node 4 y-DOF) should be 0");
        assert_eq!(f[14], 0.0, "f[14] (node 4 z-DOF) should be 0");

        // Global node 0: f[0..3]
        assert!(
            (f[0] - expected).abs() < TOL,
            "f[0] (node 0 x-DOF) = {}, expected {expected}",
            f[0]
        );
        assert_eq!(f[1], 0.0, "f[1] (node 0 y-DOF) should be 0");
        assert_eq!(f[2], 0.0, "f[2] (node 0 z-DOF) should be 0");

        // Global node 7: f[21..24]
        assert!(
            (f[21] - expected).abs() < TOL,
            "f[21] (node 7 x-DOF) = {}, expected {expected}",
            f[21]
        );
        assert_eq!(f[22], 0.0, "f[22] (node 7 y-DOF) should be 0");
        assert_eq!(f[23], 0.0, "f[23] (node 7 z-DOF) should be 0");

        // Global node 2: f[6..9]
        assert!(
            (f[6] - expected).abs() < TOL,
            "f[6] (node 2 x-DOF) = {}, expected {expected}",
            f[6]
        );
        assert_eq!(f[7], 0.0, "f[7] (node 2 y-DOF) should be 0");
        assert_eq!(f[8], 0.0, "f[8] (node 2 z-DOF) should be 0");

        // All other entries must remain zero.
        let touched: std::collections::HashSet<usize> = [0, 1, 2, 6, 7, 8, 12, 13, 14, 21, 22, 23]
            .iter()
            .cloned()
            .collect();
        for i in 0..30 {
            if !touched.contains(&i) {
                assert_eq!(f[i], 0.0, "f[{i}] should be 0.0 (not a body-force DOF)");
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
        apply_body_force(
            &mut f,
            ElementOrder::P2,
            &connectivity,
            &phys,
            [1.0, 0.0, 0.0],
        );

        // vertex: ∫ λ(2λ-1) dV = -1/120; midpoint: ∫ 4λ_a λ_b dV = 1/30 on unit ref tet
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
    // apply_body_force — repeated-call accumulation (hot-loop regression)
    // =======================================================================

    /// Pins the no-per-call-state-leakage contract for the FEA load-assembly
    /// hot path (Task 3256).
    ///
    /// The FEA load assembler calls `apply_body_force` once per element in a
    /// tight loop, accumulating into a shared `f`. After N calls, every DOF
    /// must equal exactly N times the single-call contribution. Any
    /// implementation that retained state across calls — e.g. a `static mut`
    /// or thread-local `nodal_weights` that was not zeroed before each call —
    /// would produce N² growth rather than N×.
    ///
    /// Also doubles as a hot-loop fixture in the absence of a `criterion`
    /// microbench: the crate has no `benches/` directory, so a runtime test is
    /// the closest available proxy for the assembly-loop scenario.
    #[test]
    fn apply_body_force_p2_repeated_calls_accumulate_linearly() {
        let phys = scaled_p2_phys_nodes(1.0);
        let conn: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let body_force = [1.0, 0.0, 0.0];

        // Single-call baseline.
        let mut f_one = vec![0.0_f64; 30]; // 10 nodes × 3 DOFs
        apply_body_force(&mut f_one, ElementOrder::P2, &conn, &phys, body_force);

        // 100-call accumulation into a shared load vector.
        let n_calls = 100;
        let mut f_many = vec![0.0_f64; 30];
        for _ in 0..n_calls {
            apply_body_force(&mut f_many, ElementOrder::P2, &conn, &phys, body_force);
        }

        // Each DOF must be exactly n_calls × the single-call value.
        for i in 0..30 {
            let got = f_many[i];
            let expected = n_calls as f64 * f_one[i];
            assert!(
                (got - expected).abs() < TOL * n_calls as f64,
                "DOF {i}: got {got}, expected {expected} ({n_calls} × f_one[{i}] = {})",
                f_one[i],
            );
        }
    }

    /// P1 counterpart of `apply_body_force_p2_repeated_calls_accumulate_linearly`.
    ///
    /// Exercises the P1 dispatch arm of `apply_body_force` under the same
    /// hot-loop scenario. The stack-buffer optimization (Task 3256) applies to
    /// both P1 and P2 arms; this test ensures neither arm silently retains state
    /// across calls. A future change that special-cases TetP1 with a different
    /// buffer scheme would be caught here if it introduced cross-call leakage.
    #[test]
    fn apply_body_force_p1_repeated_calls_accumulate_linearly() {
        let phys: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let conn: [usize; 4] = [0, 1, 2, 3];
        let body_force = [1.0, 0.0, 0.0];

        // Single-call baseline.
        let mut f_one = vec![0.0_f64; 12]; // 4 nodes × 3 DOFs
        apply_body_force(&mut f_one, ElementOrder::P1, &conn, &phys, body_force);

        // 100-call accumulation into a shared load vector.
        let n_calls = 100;
        let mut f_many = vec![0.0_f64; 12];
        for _ in 0..n_calls {
            apply_body_force(&mut f_many, ElementOrder::P1, &conn, &phys, body_force);
        }

        // Each DOF must be exactly n_calls × the single-call value.
        for i in 0..12 {
            let got = f_many[i];
            let expected = n_calls as f64 * f_one[i];
            assert!(
                (got - expected).abs() < TOL * n_calls as f64,
                "DOF {i}: got {got}, expected {expected} ({n_calls} × f_one[{i}] = {})",
                f_one[i],
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
            let phys: [[f64; 3]; 4] =
                [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
            let connectivity = [0usize, 1, 2, 3];
            let mut f = vec![0.0_f64; 12];
            apply_body_force(
                &mut f,
                ElementOrder::P1,
                &connectivity,
                &phys,
                [1.0, 0.0, 0.0],
            );

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
        let face_phys: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
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
                area,
                traction[alpha],
                area * traction[alpha],
            );
        }

        // Nodes 3 and 4 must be untouched.
        for i in 9..15 {
            assert_eq!(f[i], 0.0, "f[{i}] should be 0.0 (not a face node)");
        }
    }

    /// P1Tri triangle in the yz-plane (rotated out of the xy-plane). Normal
    /// points in +x. Verifies that `apply_traction_load` handles non-axis-aligned
    /// faces correctly: the computed `|t_ξ × t_η|` area element must equal 1 and
    /// conservation `Σ f = area · traction` must still hold.
    ///
    /// Vertices: `(0,0,0)`, `(0,1,0)`, `(0,0,1)` — unit right triangle in the
    /// yz-plane, area = 1/2. Each node gets area/3 = 1/6.
    #[test]
    fn apply_traction_p1tri_rotated_yz_plane_conservation() {
        let face_phys: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let conn = [0usize, 1, 2];
        let traction = [3.0, -1.0, 2.0];
        let mut f = vec![0.0_f64; 9]; // 3 nodes
        apply_traction_load(&mut f, FaceOrder::P1Tri, &conn, &face_phys, traction);

        let area = 0.5;
        let expected_per_node = area / 3.0; // 1/6

        // Each node receives area/3 of each traction component.
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

        // Conservation: sum over nodes == area * traction.
        for alpha in 0..3 {
            let total: f64 = (0..3).map(|n| f[3 * n + alpha]).sum();
            let expected_total = area * traction[alpha];
            assert!(
                (total - expected_total).abs() < TOL,
                "axis {alpha}: total = {total}, expected {expected_total}",
            );
        }
    }

    /// P1Tri with a non-contiguous, non-zero-based connectivity vector
    /// `[4, 0, 7]` into a 10-node global `f` (length 30). Verifies that the
    /// scatter step places contributions at the correct global DOF indices
    /// (`f[12..15]`, `f[0..3]`, `f[21..24]`) and leaves all other entries zero.
    ///
    /// A bug that assumed local-index == global-index (i.e. `conn = [0,1,2]`)
    /// would misplace the contributions and fail this test.
    #[test]
    fn apply_traction_p1tri_non_contiguous_connectivity_scatter() {
        let face_phys: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        // Local face nodes 0, 1, 2 → global nodes 4, 0, 7.
        let conn = [4usize, 0, 7];
        let traction = [1.0, 0.0, 0.0];
        let mut f = vec![0.0_f64; 30]; // 10 nodes
        apply_traction_load(&mut f, FaceOrder::P1Tri, &conn, &face_phys, traction);

        // Each local node gets area/3 = 1/6 of traction.
        let expected = 1.0 / 6.0;

        // Global node 4: f[12..15]
        assert!(
            (f[12] - expected).abs() < TOL,
            "f[12] (node 4 x-DOF) = {}, expected {expected}",
            f[12]
        );
        assert_eq!(f[13], 0.0, "f[13] (node 4 y-DOF) should be 0");
        assert_eq!(f[14], 0.0, "f[14] (node 4 z-DOF) should be 0");

        // Global node 0: f[0..3]
        assert!(
            (f[0] - expected).abs() < TOL,
            "f[0] (node 0 x-DOF) = {}, expected {expected}",
            f[0]
        );
        assert_eq!(f[1], 0.0, "f[1] (node 0 y-DOF) should be 0");
        assert_eq!(f[2], 0.0, "f[2] (node 0 z-DOF) should be 0");

        // Global node 7: f[21..24]
        assert!(
            (f[21] - expected).abs() < TOL,
            "f[21] (node 7 x-DOF) = {}, expected {expected}",
            f[21]
        );
        assert_eq!(f[22], 0.0, "f[22] (node 7 y-DOF) should be 0");
        assert_eq!(f[23], 0.0, "f[23] (node 7 z-DOF) should be 0");

        // All other entries must remain zero.
        let touched: std::collections::HashSet<usize> =
            [0, 1, 2, 12, 13, 14, 21, 22, 23].iter().cloned().collect();
        for i in 0..30 {
            if !touched.contains(&i) {
                assert_eq!(f[i], 0.0, "f[{i}] should be 0.0 (not a face DOF)");
            }
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

    /// P2Tri triangle in the yz-plane (rotated out of the xy-plane). Normal
    /// points in +x. Verifies that `apply_traction_load` handles non-axis-aligned
    /// P2 faces correctly: the computed `|t_ξ × t_η|` area element must equal 1
    /// and conservation `Σ f = area · traction` must still hold.
    ///
    /// Vertices: `(0,0,0)`, `(0,1,0)`, `(0,0,1)` — unit right triangle in the
    /// yz-plane, area = 1/2. For constant traction on a P2 element, vertex nodes
    /// get 0 and edge-midpoint nodes each get area/3 = 1/6 of each traction
    /// component.
    ///
    /// Mirrors `apply_traction_p1tri_rotated_yz_plane_conservation` but for
    /// the P2 arm, closing the coverage gap for non-axis-aligned P2 faces.
    #[test]
    fn apply_traction_p2tri_rotated_yz_plane_conservation() {
        let face_phys: [[f64; 3]; 6] = [
            [0.0, 0.0, 0.0], // v0
            [0.0, 1.0, 0.0], // v1
            [0.0, 0.0, 1.0], // v2
            [0.0, 0.5, 0.0], // m01
            [0.0, 0.5, 0.5], // m12
            [0.0, 0.0, 0.5], // m20
        ];
        let conn: [usize; 6] = [0, 1, 2, 3, 4, 5];
        let traction = [3.0, -1.0, 2.0];
        let mut f = vec![0.0_f64; 18]; // 6 nodes × 3 DOFs
        apply_traction_load(&mut f, FaceOrder::P2Tri, &conn, &face_phys, traction);

        // Vertex nodes (0..3) get 0 for constant traction on a P2 element.
        for node in 0..3 {
            for alpha in 0..3 {
                let got = f[3 * node + alpha];
                assert!(
                    got.abs() < TOL,
                    "vertex node {node} axis {alpha}: got {got}, expected 0",
                );
            }
        }

        // Edge-midpoint nodes (3..6) each get area/3 = 1/6 of each traction component.
        let area = 0.5_f64;
        let expected_per_midpt = area / 3.0; // 1/6
        for node in 3..6 {
            for alpha in 0..3 {
                let got = f[3 * node + alpha];
                let expected = expected_per_midpt * traction[alpha];
                assert!(
                    (got - expected).abs() < TOL,
                    "midpoint node {node} axis {alpha}: got {got}, expected {expected}",
                );
            }
        }

        // Conservation: sum over all 6 nodes == area * traction (per axis).
        for alpha in 0..3 {
            let total: f64 = (0..6).map(|n| f[3 * n + alpha]).sum();
            let expected_total = area * traction[alpha];
            assert!(
                (total - expected_total).abs() < TOL,
                "axis {alpha}: total = {total}, expected {expected_total}",
            );
        }
    }

    // =======================================================================
    // apply_traction_load — P1Quad
    // =======================================================================

    /// Unit reference quad in the xy-plane with vertices `(-1,-1,0)`,
    /// `(+1,-1,0)`, `(+1,+1,0)`, `(-1,+1,0)` traversed in the canonical
    /// Hughes/Gmsh hex8 bottom-face order (counter-clockwise viewed from +ζ).
    /// Physical area = 2·2 = 4 ⇒ each of 4 nodes receives `area/4 = 1.0` of
    /// each traction component for unit traction `(1, 2, 3)`. Untouched DOFs
    /// remain exactly 0.0.
    #[test]
    fn apply_traction_p1quad_unit_reference_quad_xy_plane() {
        let face_phys: [[f64; 3]; 4] = [
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ];
        let conn = [0usize, 1, 2, 3];
        let mut f = vec![0.0_f64; 18]; // 6 nodes (4 face + 2 untouched)
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &face_phys, [1.0, 2.0, 3.0]);

        let expected_per_node = 1.0; // area/4 = 4/4
        let traction = [1.0, 2.0, 3.0];
        for node in 0..4 {
            for alpha in 0..3 {
                let got = f[3 * node + alpha];
                let expected = expected_per_node * traction[alpha];
                assert!(
                    (got - expected).abs() < TOL,
                    "node {node} axis {alpha}: got {got}, expected {expected}",
                );
            }
        }

        // Untouched DOFs (nodes 4 and 5, indices 12..18) remain exactly 0.0.
        for i in 12..18 {
            assert_eq!(f[i], 0.0, "f[{i}] should remain 0.0 (untouched DOF)");
        }
    }

    /// P1Quad conservation contract for an arbitrary traction vector.
    ///
    /// Reuses the unit reference quad fixture (area = 4) with a non-trivial
    /// traction `(3.7, -1.2, 0.5)`. Asserts (a) per-axis conservation
    /// `Σ_node f[3·node + α] == area · traction[α]` within TOL, and (b)
    /// untouched DOFs remain exactly 0.0. Pins the conservation contract
    /// independently of the equal-lumping spot check.
    #[test]
    fn apply_traction_p1quad_conservation_arbitrary_traction() {
        let face_phys: [[f64; 3]; 4] = [
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ];
        let conn = [0usize, 1, 2, 3];
        let traction = [3.7_f64, -1.2, 0.5];
        let mut f = vec![0.0_f64; 18]; // 6 nodes (4 face + 2 untouched)
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &face_phys, traction);

        let area = 4.0_f64;

        // (a) Per-axis conservation: Σ f = area · traction.
        for alpha in 0..3 {
            let total: f64 = (0..4).map(|n| f[3 * n + alpha]).sum();
            let expected = area * traction[alpha];
            assert!(
                (total - expected).abs() < TOL,
                "axis {alpha}: total = {total}, expected {expected}",
            );
        }

        // (b) Untouched DOFs (nodes 4, 5) remain exactly 0.0.
        for i in 12..18 {
            assert_eq!(f[i], 0.0, "f[{i}] should remain 0.0 (untouched DOF)");
        }
    }

    /// P1Quad in the yz-plane (rotated out of the xy-plane). Outward normal
    /// points in +x. Vertices `(0,-1,-1)`, `(0,+1,-1)`, `(0,+1,+1)`, `(0,-1,+1)`
    /// in canonical CCW-from-outside order, area = 4. Apply traction
    /// `(3.0, -1.0, 2.0)`. Asserts (a) each node receives `area/4 = 1.0` of
    /// each traction component, (b) per-axis conservation
    /// `Σ f = area · traction` holds. Exercises the `|t_ξ × t_η|`
    /// cross-product on a non-axis-aligned face — mirrors the existing
    /// `apply_traction_p1tri_rotated_yz_plane_conservation`.
    #[test]
    fn apply_traction_p1quad_rotated_yz_plane_conservation() {
        let face_phys: [[f64; 3]; 4] = [
            [0.0, -1.0, -1.0],
            [0.0, 1.0, -1.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let conn = [0usize, 1, 2, 3];
        let traction = [3.0_f64, -1.0, 2.0];
        let mut f = vec![0.0_f64; 12]; // 4 nodes × 3 DOFs
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &face_phys, traction);

        let area = 4.0_f64;
        let expected_per_node = area / 4.0; // 1.0

        // (a) Each node gets area/4 = 1.0 of each traction component.
        for node in 0..4 {
            for alpha in 0..3 {
                let got = f[3 * node + alpha];
                let expected = expected_per_node * traction[alpha];
                assert!(
                    (got - expected).abs() < TOL,
                    "node {node} axis {alpha}: got {got}, expected {expected}",
                );
            }
        }

        // (b) Per-axis conservation.
        for alpha in 0..3 {
            let total: f64 = (0..4).map(|n| f[3 * n + alpha]).sum();
            let expected_total = area * traction[alpha];
            assert!(
                (total - expected_total).abs() < TOL,
                "axis {alpha}: total = {total}, expected {expected_total}",
            );
        }
    }

    /// Affinely-mapped quad — a parallelogram in xy with vertices
    /// `(0,0,0)`, `(2,0,0)`, `(2.5,1,0)`, `(0.5,1,0)`. Base × height = 2 × 1
    /// ⇒ phys_area = 2. Unit traction along +x. Asserts (a) each of 4 nodes
    /// receives `area/4 = 0.5` along x within TOL, (b) all y- and z-DOFs are
    /// exactly 0.0, (c) conservation `Σ f_x = 2.0`. Pins that an affine map
    /// (constant Jacobian) preserves equal-lumping for P1 quads, independent
    /// of the rectangular special case.
    #[test]
    fn apply_traction_p1quad_sheared_parallelogram_lumps_evenly() {
        let face_phys: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.5, 1.0, 0.0],
            [0.5, 1.0, 0.0],
        ];
        let conn = [0usize, 1, 2, 3];
        let traction = [1.0_f64, 0.0, 0.0];
        let mut f = vec![0.0_f64; 12]; // 4 nodes × 3 DOFs
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &face_phys, traction);

        let area = 2.0_f64;
        let expected_per_node_x = area / 4.0; // 0.5

        // (a) Each node receives area/4 = 0.5 along x within TOL.
        for node in 0..4 {
            let got = f[3 * node];
            assert!(
                (got - expected_per_node_x).abs() < TOL,
                "node {node} x-DOF: got {got}, expected {expected_per_node_x}",
            );
        }

        // (b) All y- and z-DOFs are exactly 0.0.
        for node in 0..4 {
            assert_eq!(f[3 * node + 1], 0.0, "node {node} y-DOF should be 0.0");
            assert_eq!(f[3 * node + 2], 0.0, "node {node} z-DOF should be 0.0");
        }

        // (c) Conservation: Σ f_x = 2.0.
        let total_x: f64 = (0..4).map(|n| f[3 * n]).sum();
        assert!(
            (total_x - area).abs() < TOL,
            "total f_x = {total_x}, expected {area}",
        );
    }

    /// P1Quad with non-contiguous, non-zero-based connectivity `[4, 0, 7, 9]`
    /// into a 10-node global `f` (length 30). Each local node receives
    /// `area/4 = 1.0` along x (traction = +x), area = 4 on the canonical
    /// unit reference quad in the xy-plane. Verifies that the scatter step
    /// places contributions at the correct global DOF indices and leaves
    /// all other entries zero. Mirrors `apply_traction_p1tri_non_contiguous_connectivity_scatter`.
    #[test]
    fn apply_traction_p1quad_non_contiguous_connectivity_scatter() {
        let face_phys: [[f64; 3]; 4] = [
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ];
        // Local face nodes 0, 1, 2, 3 → global nodes 4, 0, 7, 9.
        let conn = [4usize, 0, 7, 9];
        let traction = [1.0_f64, 0.0, 0.0];
        let mut f = vec![0.0_f64; 30]; // 10 nodes
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &face_phys, traction);

        let expected = 1.0_f64; // area/4 = 4/4

        // Global node 4: f[12..15]
        assert!(
            (f[12] - expected).abs() < TOL,
            "f[12] (node 4 x-DOF) = {}, expected {expected}",
            f[12]
        );
        assert_eq!(f[13], 0.0, "f[13] (node 4 y-DOF) should be 0");
        assert_eq!(f[14], 0.0, "f[14] (node 4 z-DOF) should be 0");

        // Global node 0: f[0..3]
        assert!(
            (f[0] - expected).abs() < TOL,
            "f[0] (node 0 x-DOF) = {}, expected {expected}",
            f[0]
        );
        assert_eq!(f[1], 0.0, "f[1] (node 0 y-DOF) should be 0");
        assert_eq!(f[2], 0.0, "f[2] (node 0 z-DOF) should be 0");

        // Global node 7: f[21..24]
        assert!(
            (f[21] - expected).abs() < TOL,
            "f[21] (node 7 x-DOF) = {}, expected {expected}",
            f[21]
        );
        assert_eq!(f[22], 0.0, "f[22] (node 7 y-DOF) should be 0");
        assert_eq!(f[23], 0.0, "f[23] (node 7 z-DOF) should be 0");

        // Global node 9: f[27..30]
        assert!(
            (f[27] - expected).abs() < TOL,
            "f[27] (node 9 x-DOF) = {}, expected {expected}",
            f[27]
        );
        assert_eq!(f[28], 0.0, "f[28] (node 9 y-DOF) should be 0");
        assert_eq!(f[29], 0.0, "f[29] (node 9 z-DOF) should be 0");

        // All other entries must remain zero.
        let touched: std::collections::HashSet<usize> = [
            0, 1, 2, 12, 13, 14, 21, 22, 23, 27, 28, 29,
        ]
        .iter()
        .cloned()
        .collect();
        for i in 0..30 {
            if !touched.contains(&i) {
                assert_eq!(f[i], 0.0, "f[{i}] should be 0.0 (not a face DOF)");
            }
        }
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
        let phys: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let conn = [0usize, 1, 99]; // 99 out of range for f.len()/3 = 3
        let mut f = vec![0.0_f64; 9];
        apply_traction_load(&mut f, FaceOrder::P1Tri, &conn, &phys, [0.0; 3]);
    }

    // P1Quad contract-panic tests — mirror the existing P1Tri / P2Tri set.

    #[test]
    #[should_panic(expected = "connectivity.len()")]
    fn apply_traction_p1quad_wrong_connectivity_len() {
        let phys: [[f64; 3]; 4] = [[0.0; 3]; 4];
        let conn = [0usize, 1, 2]; // 3 instead of 4
        let mut f = vec![0.0_f64; 12];
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "phys_nodes.len()")]
    fn apply_traction_p1quad_wrong_phys_nodes_len() {
        let phys: [[f64; 3]; 3] = [[0.0; 3]; 3]; // 3 instead of 4
        let conn = [0usize, 1, 2, 3];
        let mut f = vec![0.0_f64; 12];
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "f.len() = 7")]
    fn apply_traction_p1quad_f_len_not_multiple_of_3() {
        let phys: [[f64; 3]; 4] = [[0.0; 3]; 4];
        let conn = [0usize, 1, 2, 3];
        let mut f = vec![0.0_f64; 7];
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &phys, [0.0; 3]);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn apply_traction_p1quad_connectivity_out_of_range() {
        let phys: [[f64; 3]; 4] = [
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ];
        let conn = [0usize, 1, 2, 99]; // 99 out of range for f.len()/3 = 4
        let mut f = vec![0.0_f64; 12];
        apply_traction_load(&mut f, FaceOrder::P1Quad, &conn, &phys, [0.0; 3]);
    }

    /// Second call accumulates rather than overwrites (`+=` semantics)
    /// for the P1Quad arm — pins that `integrate_face_generic`'s
    /// scatter step uses `+=` not `=` so two sequential applies of the
    /// same traction produce a result exactly 2× the single-call value.
    #[test]
    fn apply_traction_p1quad_accumulates_on_second_call() {
        let face_phys: [[f64; 3]; 4] = [
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ];
        let conn = [0usize, 1, 2, 3];
        let traction = [1.0_f64, 2.0, 3.0];

        let mut f_one = vec![0.0_f64; 12];
        apply_traction_load(&mut f_one, FaceOrder::P1Quad, &conn, &face_phys, traction);

        let mut f_two = vec![0.0_f64; 12];
        apply_traction_load(&mut f_two, FaceOrder::P1Quad, &conn, &face_phys, traction);
        apply_traction_load(&mut f_two, FaceOrder::P1Quad, &conn, &face_phys, traction);

        for i in 0..12 {
            let expected = 2.0 * f_one[i];
            assert_eq!(
                f_two[i].to_bits(),
                expected.to_bits(),
                "DOF {i}: f_two = {} but expected 2× f_one = {expected}",
                f_two[i],
            );
        }
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
