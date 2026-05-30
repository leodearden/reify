//! Test-only helpers shared between the `assembly::*` test modules and
//! the integration tests under `crates/reify-solver-elastic/tests/`.
//!
//! Lives under `#[doc(hidden)] pub mod test_support;` in
//! [`crate::assembly`] so both in-crate unit tests and external
//! integration tests can pull from a single source of truth. Putting
//! the shared helpers in one place keeps the EDGES traversal driven
//! directly off [`crate::elements::tet_p2::EDGES`] (the production
//! constant), so a reordering of edges in production can never silently
//! desynchronise the test fixtures from the indexing the assembly code
//! expects.
//!
//! # Visibility
//!
//! Fixture-geometry helpers (`scaled_*_phys_nodes`,
//! `dimensionless_steel_like`) are `pub` so external integration tests
//! can import them; analysis helpers (`matvec`, `linf`,
//! `strain_energies`, `ElementStiffnessTestSpec`,
//! `run_element_stiffness_tests`) stay `pub(crate)` because only
//! in-crate unit tests exercise them. The module-level
//! `#![allow(dead_code)]` is required because all of these items are
//! only reachable from `#[cfg(test)]` contexts (in-crate unit tests +
//! integration tests), so in a plain `cargo build` the dead-code lint
//! would otherwise flag the `pub(crate)` helpers.

#![allow(dead_code)]

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;
use crate::elements::tet_p2::EDGES;

/// Steel-like dimensionless material: E = 1, ν = 0.3.
pub fn dimensionless_steel_like() -> IsotropicElastic {
    IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    }
}

/// Compute K · u for a row-major `ElementStiffness`.
pub(crate) fn matvec(k: &ElementStiffness, u: &[f64]) -> Vec<f64> {
    assert_eq!(k.n_dofs, u.len());
    let n = k.n_dofs;
    let mut out = vec![0.0; n];
    for (i, out_i) in out.iter_mut().enumerate() {
        for (j, &u_j) in u.iter().enumerate() {
            *out_i += k.get(i, j) * u_j;
        }
    }
    out
}

/// L∞ norm of a slice.
pub(crate) fn linf(v: &[f64]) -> f64 {
    v.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()))
}

/// Relative-tolerance assert for floating-point values.
///
/// Asserts `|lhs − rhs| < tol · scale` where
/// `scale = lhs.abs().max(rhs.abs()).max(1.0)`.
///
/// This is the single source of truth for the relative-tolerance convention
/// used across bar, tet, and integration-test modules. Hoisted here so that
/// changes to the tolerance logic propagate uniformly without per-module drift.
pub fn assert_close(lhs: f64, rhs: f64, tol: f64, label: &str) {
    let scale = lhs.abs().max(rhs.abs()).max(1.0);
    assert!(
        (lhs - rhs).abs() < tol * scale,
        "{label}: |{lhs} − {rhs}| = {} ≥ tol·scale = {}",
        (lhs - rhs).abs(),
        tol * scale,
    );
}

/// Compute U_K = 0.5 · uᵀ K u and U_analytical = 0.5 · εᵀ D ε · V.
pub(crate) fn strain_energies(
    k: &ElementStiffness,
    u: &[f64],
    eps_voigt: &[f64; 6],
    d: &[[f64; 6]; 6],
    volume: f64,
) -> (f64, f64) {
    let ku = matvec(k, u);
    let mut u_dot_ku = 0.0;
    for i in 0..u.len() {
        u_dot_ku += u[i] * ku[i];
    }
    let u_k = 0.5 * u_dot_ku;

    let mut d_eps = [0.0_f64; 6];
    for i in 0..6 {
        for j in 0..6 {
            d_eps[i] += d[i][j] * eps_voigt[j];
        }
    }
    let mut eps_dot_d_eps = 0.0;
    for i in 0..6 {
        eps_dot_d_eps += eps_voigt[i] * d_eps[i];
    }
    (u_k, 0.5 * eps_dot_d_eps * volume)
}

/// Shape parameters for the generic element-stiffness behavioral suite.
///
/// Wrapped in a struct rather than passed positionally so call sites use
/// field-labeled literals (unlabeled scalars conflate too easily).
///
/// `n_dofs` is intentionally absent: it is always `3 * n_nodes` for a
/// 3-axis displacement element, so it is derived inside the helper rather
/// than repeated at every call site. This makes the invariant structurally
/// unrepresentable rather than asserted.
pub(crate) struct ElementStiffnessTestSpec {
    /// Number of nodes in the element (8 for hex P1, 6 for wedge P1).
    pub n_nodes: usize,
    /// Physical volume at scale s = 1 (8.0 for hex [−1,1]³, 1.0 for wedge reference prism).
    pub vol_ref: f64,
    /// Centroid of the unit fixture (used by the RB-rotation null-space test).
    pub centroid: [f64; 3],
    /// `(i, j)` node indices to swap to produce a left-handed fixture.
    pub swap_pair: (usize, usize),
    /// Effective quadrature volume of the swapped (left-handed) element.
    pub vol_swapped: f64,
}

/// Run the 7 behavioral tests common to any P1 hex/wedge-class element:
/// symmetry, rigid-body translation/rotation null spaces, normal-strain and
/// full-6-component patch tests, volume scaling, and left-handed orientation.
///
/// # Parameters
/// - `compute_k`: stiffness entry point wrapped as `&[[f64;3]] × &IsotropicElastic → ElementStiffness`.
/// - `make_phys`: returns the canonical fixture at scale `s` as a `Vec<[f64;3]>`.
/// - `spec`: shape parameters for the element (DOF count, node count, volume, centroid, swap pair).
#[allow(clippy::needless_range_loop)]
pub(crate) fn run_element_stiffness_tests(
    compute_k: &dyn Fn(&[[f64; 3]], &IsotropicElastic) -> ElementStiffness,
    make_phys: &dyn Fn(f64) -> Vec<[f64; 3]>,
    spec: ElementStiffnessTestSpec,
) {
    let ElementStiffnessTestSpec {
        n_nodes,
        vol_ref,
        centroid,
        swap_pair,
        vol_swapped,
    } = spec;
    let n_dofs = 3 * n_nodes;
    let mat = dimensionless_steel_like();
    let phys1 = make_phys(1.0);
    assert_eq!(
        phys1.len(),
        n_nodes,
        "phys1.len() must equal n_nodes (got phys1.len()={}, n_nodes={})",
        phys1.len(),
        n_nodes,
    );
    let k = compute_k(&phys1, &mat);

    // (b) Symmetry
    for i in 0..n_dofs {
        for j in 0..n_dofs {
            let lhs = k.get(i, j);
            let rhs = k.get(j, i);
            let scale = lhs.abs().max(rhs.abs()).max(1.0);
            assert!(
                (lhs - rhs).abs() < 1e-9 * scale,
                "symmetry [{i},{j}]: K[i][j]={lhs} K[j][i]={rhs}",
            );
        }
    }

    // (c) Rigid-body translation null space
    for axis in 0..3 {
        let mut u = vec![0.0; n_dofs];
        for node in 0..n_nodes {
            u[3 * node + axis] = 1.0;
        }
        let ku = matvec(&k, &u);
        assert!(
            linf(&ku) < 1e-9,
            "RB-trans axis {axis}: ‖K·u‖_∞ = {} (expected <1e-9)",
            linf(&ku),
        );
    }

    // (d) Rigid-body rotation null space (about centroid)
    for axis in 0..3 {
        let mut omega = [0.0_f64; 3];
        omega[axis] = 1.0;
        let mut u = vec![0.0; n_dofs];
        for (node, x) in phys1.iter().enumerate() {
            let dx = [x[0] - centroid[0], x[1] - centroid[1], x[2] - centroid[2]];
            u[3 * node] = omega[1] * dx[2] - omega[2] * dx[1];
            u[3 * node + 1] = omega[2] * dx[0] - omega[0] * dx[2];
            u[3 * node + 2] = omega[0] * dx[1] - omega[1] * dx[0];
        }
        let ku = matvec(&k, &u);
        assert!(
            linf(&ku) < 1e-9,
            "RB-rot axis {axis}: ‖K·u‖_∞ = {} (expected <1e-9)",
            linf(&ku),
        );
    }

    let d_mat = mat.d_matrix();

    // (e) Normal-strain patch test: u(x) = diag(a,b,c)·x
    {
        let (a, b, c) = (0.01_f64, -0.005, 0.003);
        let mut u = vec![0.0; n_dofs];
        for (ni, x) in phys1.iter().enumerate() {
            u[3 * ni] = a * x[0];
            u[3 * ni + 1] = b * x[1];
            u[3 * ni + 2] = c * x[2];
        }
        let eps = [a, b, c, 0.0, 0.0, 0.0];
        let (u_k, u_a) = strain_energies(&k, &u, &eps, &d_mat, vol_ref);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "normal-strain patch: U_K={u_k} U_analytical={u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    // (f) Full 6-component patch test: u(x) = A·x with A symmetric
    {
        let (a, b, c, dv, ev, fv) = (0.01_f64, -0.005, 0.003, 0.002, -0.001, 0.0007);
        let big_a = [
            [a, dv / 2.0, fv / 2.0],
            [dv / 2.0, b, ev / 2.0],
            [fv / 2.0, ev / 2.0, c],
        ];
        let mut u = vec![0.0; n_dofs];
        for (ni, x) in phys1.iter().enumerate() {
            for i in 0..3 {
                let mut s = 0.0;
                for j in 0..3 {
                    s += big_a[i][j] * x[j];
                }
                u[3 * ni + i] = s;
            }
        }
        let eps = [a, b, c, dv, ev, fv];
        let (u_k, u_a) = strain_energies(&k, &u, &eps, &d_mat, vol_ref);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "full-6-component patch: U_K={u_k} U_analytical={u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    // (g) Volume scaling: K(2s) == 2·K(s) entrywise
    {
        let k2 = compute_k(&make_phys(2.0), &mat);
        for i in 0..n_dofs {
            for j in 0..n_dofs {
                let unit = k.get(i, j);
                let got = k2.get(i, j);
                let expected = 2.0 * unit;
                let scale = expected.abs().max(unit.abs()).max(1.0);
                assert!(
                    (got - expected).abs() < 1e-9 * scale,
                    "vol-scaling [{i},{j}]: K(2s)={got} expected 2·K(s)={expected}",
                );
            }
        }
    }

    // (h) Left-handed orientation: normal-strain patch test with swapped nodes
    {
        let (a, b, c) = (0.01_f64, -0.005, 0.003);
        let mut phys_lh = make_phys(1.0);
        phys_lh.swap(swap_pair.0, swap_pair.1);
        let k_lh = compute_k(&phys_lh, &mat);
        let mut u = vec![0.0; n_dofs];
        for (ni, x) in phys_lh.iter().enumerate() {
            u[3 * ni] = a * x[0];
            u[3 * ni + 1] = b * x[1];
            u[3 * ni + 2] = c * x[2];
        }
        let eps = [a, b, c, 0.0, 0.0, 0.0];
        let (u_k, u_a) = strain_energies(&k_lh, &u, &eps, &d_mat, vol_swapped);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "left-handed patch: U_K={u_k} U_analytical={u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
        assert!(u_k > 0.0, "left-handed: expected U_K > 0, got {u_k}");
    }
}

/// Build the 6 physical nodes of a scaled unit wedge in canonical Gmsh PRI6 order:
/// bottom triangle (ζ = −s) first, then top triangle (ζ = +s) in the same barycentric
/// order (`L₀, L₁, L₂`).
///
/// `s = 1.0` recovers the canonical reference prism (unit triangle × [−1, +1]);
/// other scales are used by the volume-scaling tests.
pub fn scaled_unit_wedge_phys_nodes(s: f64) -> [[f64; 3]; 6] {
    [
        [0.0, 0.0, -s], // node 0: L₀, ζ = −1  → (0, 0, −s)
        [s, 0.0, -s],   // node 1: L₁, ζ = −1  → (s, 0, −s)
        [0.0, s, -s],   // node 2: L₂, ζ = −1  → (0, s, −s)
        [0.0, 0.0, s],  // node 3: L₀, ζ = +1  → (0, 0, +s)
        [s, 0.0, s],    // node 4: L₁, ζ = +1  → (s, 0, +s)
        [0.0, s, s],    // node 5: L₂, ζ = +1  → (0, s, +s)
    ]
}

/// Build the 8 physical nodes of a scaled unit hex in canonical Hughes/Gmsh hex8
/// order: bottom face (ζ = −s) counter-clockwise when viewed from +ζ, then
/// top face (ζ = +s) in the same cyclic order.
///
/// `s = 1.0` recovers the canonical reference cube `[−1, 1]³`; other scales are
/// used by the volume-scaling tests.
pub fn scaled_unit_hex_phys_nodes(s: f64) -> [[f64; 3]; 8] {
    [
        [-s, -s, -s], // node 0: (ξ,η,ζ) = (−1,−1,−1)
        [s, -s, -s],  // node 1: (+1,−1,−1)
        [s, s, -s],   // node 2: (+1,+1,−1)
        [-s, s, -s],  // node 3: (−1,+1,−1)
        [-s, -s, s],  // node 4: (−1,−1,+1)
        [s, -s, s],   // node 5: (+1,−1,+1)
        [s, s, s],    // node 6: (+1,+1,+1)
        [-s, s, s],   // node 7: (−1,+1,+1)
    ]
}

/// Build the canonical 10-node P2 phys-node layout for a uniformly scaled
/// reference tet: 4 vertices at `(0,0,0), (s,0,0), (0,s,0), (0,0,s)`
/// followed by the 6 edge-midpoint nodes in the production
/// [`crate::elements::tet_p2::EDGES`] order.
///
/// `s = 1.0` recovers the canonical unit reference tet; other scales are
/// used by the volume-scaling tests.
pub fn scaled_p2_phys_nodes(s: f64) -> [[f64; 3]; 10] {
    let v: [[f64; 3]; 4] = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
    let mid = |a: usize, b: usize| {
        [
            0.5 * (v[a][0] + v[b][0]),
            0.5 * (v[a][1] + v[b][1]),
            0.5 * (v[a][2] + v[b][2]),
        ]
    };

    let mut nodes = [[0.0_f64; 3]; 10];
    for (i, vert) in v.iter().enumerate() {
        nodes[i] = *vert;
    }
    // Drive midpoints off the production EDGES table — never re-list the
    // pairs as literals here, so an off-by-one in EDGES surfaces as a
    // production-test mismatch rather than silently aligning.
    for (i, &(a, b)) in EDGES.iter().enumerate() {
        nodes[4 + i] = mid(a, b);
    }
    nodes
}

/// Promote a P1 tetrahedral mesh to P2 by inserting edge-midpoint nodes.
///
/// For each unique edge `(a, b)` in the P1 mesh (determined by the set of
/// edges across all tets in canonical `EDGES` order), inserts a single
/// midpoint node at `(nodes_p1[a] + nodes_p1[b]) / 2`. Adjacent tets sharing
/// an edge reference the **same** midpoint node id — deduplication is done via
/// a `HashMap` keyed by the sorted `(min(a,b), max(a,b))` corner-node-id pair.
///
/// The returned 10-node P2 connectivity follows the canonical Hughes/Gmsh
/// ordering used throughout this crate: indices 0..=3 are the four original
/// P1 corner nodes, indices 4..=9 are the edge-midpoint nodes for
/// [`crate::elements::tet_p2::EDGES`]\[0..=5\] in that order.
///
/// # Purpose
///
/// Single source of truth for P1 → P2 mesh promotion shared between
/// `tests/kg_p2_tet.rs` (kernel-level K_g accuracy tests) and the P2
/// Euler-column pipeline test in `tests/euler_column_pin_pin.rs`. Both files
/// compile as separate binaries and cannot share a Rust module; routing
/// through `test_support.rs` (which is `pub` from `assembly::test_support`)
/// avoids drift.
pub fn promote_tets_to_p2(
    nodes_p1: &[[f64; 3]],
    tets_p1: &[[usize; 4]],
) -> (Vec<[f64; 3]>, Vec<[usize; 10]>) {
    use std::collections::HashMap;

    let mut nodes_p2: Vec<[f64; 3]> = nodes_p1.to_vec();
    // Keyed by (min, max) corner id → midpoint node index in nodes_p2.
    let mut edge_to_mid: HashMap<(usize, usize), usize> = HashMap::new();
    let mut tets_p2: Vec<[usize; 10]> = Vec::with_capacity(tets_p1.len());

    for tet in tets_p1 {
        let mut p2_tet = [0usize; 10];
        // First 4 entries: copy P1 corner node indices verbatim.
        p2_tet[..4].copy_from_slice(tet);

        // Entries 4..10: edge-midpoint node indices in EDGES order.
        for (edge_idx, &(a, b)) in EDGES.iter().enumerate() {
            let ca = tet[a]; // global corner node id for local vertex a
            let cb = tet[b]; // global corner node id for local vertex b
            let key = (ca.min(cb), ca.max(cb));
            // `or_insert_with` allocates the midpoint only on the first
            // encounter; subsequent tets sharing the same global edge reuse it.
            let mid_idx = *edge_to_mid.entry(key).or_insert_with(|| {
                let pa = nodes_p1[ca];
                let pb = nodes_p1[cb];
                let idx = nodes_p2.len();
                nodes_p2.push([
                    0.5 * (pa[0] + pb[0]),
                    0.5 * (pa[1] + pb[1]),
                    0.5 * (pa[2] + pb[2]),
                ]);
                idx
            });
            p2_tet[4 + edge_idx] = mid_idx;
        }
        tets_p2.push(p2_tet);
    }

    (nodes_p2, tets_p2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::tet_p2::EDGES as TET_P2_EDGES;

    /// Six-tet long-diagonal decomposition of a unit brick (identical to the
    /// decomposition used in `tests/kg_p1_tet.rs` and `tests/euler_column_pin_pin.rs`).
    const TET_DECOMP: [[usize; 4]; 6] = [
        [0, 1, 2, 6],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 7, 4, 6],
        [0, 4, 5, 6],
        [0, 5, 1, 6],
    ];

    #[test]
    fn promote_tets_to_p2_single_brick_yields_shared_midpoints() {
        // Nodes of a unit cube, indexed 0-7 in the same order used by
        // `TET_DECOMP` (matches `ColumnGrid::build_node_xyz` for a 1×1×1 grid).
        let nodes_p1: &[[f64; 3]] = &[
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ];
        let tets_p1: &[[usize; 4]] = &TET_DECOMP;

        let (nodes_p2, tets_p2) = promote_tets_to_p2(nodes_p1, tets_p1);

        // (a) P2 node count = corner count + number of unique edges across all tets.
        {
            use std::collections::HashSet;
            let mut unique_edges: HashSet<(usize, usize)> = HashSet::new();
            for tet in tets_p1 {
                for &(a, b) in TET_P2_EDGES.iter() {
                    let ca = tet[a];
                    let cb = tet[b];
                    unique_edges.insert((ca.min(cb), ca.max(cb)));
                }
            }
            let n_unique_edges = unique_edges.len();
            assert_eq!(
                nodes_p2.len(),
                nodes_p1.len() + n_unique_edges,
                "P2 node count must equal P1 corners ({}) + unique edges ({})",
                nodes_p1.len(),
                n_unique_edges,
            );
        }

        // (b) Two tets sharing a P1 edge must reference the same midpoint node id.
        //
        // Tets 0 [0,1,2,6] and 1 [0,2,3,6] share the global edge (0,2):
        //   tet 0: EDGES[2] = (2,0) → local vertices tet[2]=2, tet[0]=0 → global (0,2) → p2_tet[4+2]
        //   tet 1: EDGES[0] = (0,1) → local vertices tet[0]=0, tet[1]=2 → global (0,2) → p2_tet[4+0]
        assert_eq!(
            tets_p2[0][4 + 2],
            tets_p2[1][4 + 0],
            "tets 0 and 1 share the P1 edge (0,2); their midpoint node ids must match",
        );

        // Tets 0 and 1 also share edge (0,6):
        //   tet 0: EDGES[3] = (0,3) → tet[0]=0, tet[3]=6 → global (0,6) → p2_tet[4+3]
        //   tet 1: EDGES[3] = (0,3) → tet[0]=0, tet[3]=6 → global (0,6) → p2_tet[4+3]
        assert_eq!(
            tets_p2[0][4 + 3],
            tets_p2[1][4 + 3],
            "tets 0 and 1 share the P1 edge (0,6); their midpoint node ids must match",
        );

        // Tets 1 [0,2,3,6] and 2 [0,3,7,6] share edge (0,3):
        //   tet 1: EDGES[2] = (2,0) → tet[2]=3, tet[0]=0 → global (0,3) → p2_tet[4+2]
        //   tet 2: EDGES[0] = (0,1) → tet[0]=0, tet[1]=3 → global (0,3) → p2_tet[4+0]
        assert_eq!(
            tets_p2[1][4 + 2],
            tets_p2[2][4],
            "tets 1 and 2 share the P1 edge (0,3); their midpoint node ids must match",
        );

        // (c) Each P2 tet has exactly 10 node indices, all distinct.
        for (t_idx, p2_tet) in tets_p2.iter().enumerate() {
            use std::collections::HashSet;
            let ids: HashSet<usize> = p2_tet.iter().copied().collect();
            assert_eq!(
                ids.len(),
                10,
                "P2 tet {t_idx} must have 10 distinct node ids (got {} unique)",
                ids.len(),
            );
        }

        // (d) All P2 midpoint node positions lie on the straight-line midpoint
        //     of their two parent corners.  Verify for the (0,2) midpoint
        //     (shared between tets 0 and 1).
        let mid_id = tets_p2[0][4 + 2];
        let expected_mid = [
            0.5 * (nodes_p1[0][0] + nodes_p1[2][0]),
            0.5 * (nodes_p1[0][1] + nodes_p1[2][1]),
            0.5 * (nodes_p1[0][2] + nodes_p1[2][2]),
        ];
        for k in 0..3 {
            assert!(
                (nodes_p2[mid_id][k] - expected_mid[k]).abs() < 1e-12,
                "midpoint of edge (0,2) coord[{k}] = {} expected {}",
                nodes_p2[mid_id][k],
                expected_mid[k],
            );
        }
    }

    #[test]
    #[should_panic(expected = "phys1.len() must equal n_nodes")]
    fn run_element_stiffness_tests_panics_on_phys_length_mismatch() {
        // spec is internally consistent; make_phys returns 5 nodes instead of 6.
        let spec = ElementStiffnessTestSpec {
            n_nodes: 6,
            vol_ref: 1.0,
            centroid: [0.0; 3],
            swap_pair: (0, 1),
            vol_swapped: 1.0,
        };
        run_element_stiffness_tests(
            &|_, _| unreachable!("compute_k must not be called when phys1.len() is wrong"),
            &|_| vec![[0.0_f64; 3]; 5], // wrong: 5 nodes instead of 6
            spec,
        );
    }
}
