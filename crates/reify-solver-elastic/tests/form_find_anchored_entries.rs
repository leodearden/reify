//! The public-entry contract of the anchored force-density form-finding
//! entries — `form_find_anchored`, `form_find_anchored_surfaces` and
//! `form_find_anchored_surfaces_aniso` — driven from the crate root only: the
//! analytic FD-identity goldens and the force-density echo, the empty-free-set
//! and singular-reduced-stiffness verdicts, the isolated-free-node rejections,
//! the surface entry's line-only equivalence and the anisotropic entry's
//! isotropic equivalence. The input contract these entries share is
//! `tests/force_density_input.rs`. Moved from `form_find.rs`'s unit tests by
//! #9275; the stencil and principal-stress tests stay there.

use reify_solver_elastic::{
    AnisoFormFindError, AnisotropicSurfaceStress, FormFindError, MemberKind, form_find_anchored,
    form_find_anchored_surfaces, form_find_anchored_surfaces_aniso,
};
use reify_test_support::tensegrity_fixtures::{TENT_ANCHORS, TENT_NODE_COORDS, TENT_TRIS};

/// A membrane test case: `(nodes, surface triangles, anchor indices)`.
/// Aliased to keep the surface-test helper signatures readable (and to
/// silence `clippy::type_complexity` on the bare nested-tuple return).
type MembraneCase = (Vec<[f64; 3]>, Vec<(usize, usize, usize)>, Vec<usize>);

/// Tolerance for the analytic FD-identity goldens. The reduced linear solve
/// reproduces these exact identities to ~1e-13; 1e-9 leaves ~4 orders of
/// margin while still catching a wrong solve.
const TOL: f64 = 1e-9;

/// Max absolute componentwise difference between two 3-vectors.
fn max_coord_err(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

// (a) A single free node cabled to 4 anchors with equal force density solves
// to the (unweighted) centroid of the anchors — the weighted-centroid FD
// identity x_f = Σ qᵢ x_{aᵢ} / Σ qᵢ with all qᵢ equal. Anchors are placed
// symmetrically in x,y so the centroid is (0, 0, 0.5).
#[test]
fn single_free_node_equal_q_solves_to_anchor_centroid() {
    let nodes = vec![
        [0.3, 0.2, 0.4], // free node 0 — deliberately off-solution
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
    ];
    let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
    let kinds = [MemberKind::Cable; 4];
    let q = [1.0, 1.0, 1.0, 1.0];
    let anchors = [1, 2, 3, 4];

    let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
        .expect("equal-q anchored cable net must be feasible");

    let expected = [0.0, 0.0, 0.5];
    assert!(
        max_coord_err(solve.nodes[0], expected) < TOL,
        "nodes[0] = {:?}, expected anchor centroid {:?}",
        solve.nodes[0],
        expected,
    );
}

// (b) Unequal force densities give the *weighted* centroid
// x_f = Σ qᵢ x_{aᵢ} / Σ qᵢ. Same geometry as (a) but q = [2,1,1,1]; the
// expected point is computed from the identity rather than hard-coded.
#[test]
fn single_free_node_unequal_q_solves_to_weighted_centroid() {
    let anchor_pts = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
    ];
    let nodes = vec![
        [0.3, 0.2, 0.4], // free node 0
        anchor_pts[0],
        anchor_pts[1],
        anchor_pts[2],
        anchor_pts[3],
    ];
    let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
    let kinds = [MemberKind::Cable; 4];
    let q = [2.0, 1.0, 1.0, 1.0];
    let anchors = [1, 2, 3, 4];

    // Analytic weighted centroid Σ qᵢ x_i / Σ qᵢ.
    let qsum: f64 = q.iter().sum();
    let mut expected = [0.0_f64; 3];
    for (w, p) in q.iter().zip(anchor_pts.iter()) {
        for (e, c) in expected.iter_mut().zip(p.iter()) {
            *e += w * c;
        }
    }
    for e in expected.iter_mut() {
        *e /= qsum;
    }

    let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
        .expect("unequal-q anchored cable net must be feasible");

    assert!(
        max_coord_err(solve.nodes[0], expected) < TOL,
        "nodes[0] = {:?}, expected weighted centroid {:?}",
        solve.nodes[0],
        expected,
    );
}

// (c) Two free nodes in a uniform-tension chain
// anchor(x=0) — node0 — node1 — anchor(x=3), all cables q=1. The interior
// nodes settle to evenly-spaced positions x0=1, x1=2. This exercises the
// off-diagonal D_ff coupling: the node0–node1 cable couples the two free
// equations, so a diagonal-only solve would get this wrong.
#[test]
fn two_free_node_chain_solves_to_uniform_spacing() {
    let nodes = vec![
        [0.5, 0.0, 0.0], // free node 0 — off-solution
        [2.5, 0.0, 0.0], // free node 1 — off-solution
        [0.0, 0.0, 0.0], // anchor at x=0
        [3.0, 0.0, 0.0], // anchor at x=3
    ];
    let members = [(2, 0), (0, 1), (1, 3)];
    let kinds = [MemberKind::Cable; 3];
    let q = [1.0, 1.0, 1.0];
    let anchors = [2, 3];

    let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
        .expect("uniform-tension chain must be feasible");

    assert!(
        max_coord_err(solve.nodes[0], [1.0, 0.0, 0.0]) < TOL,
        "free node 0 = {:?}, expected (1,0,0)",
        solve.nodes[0],
    );
    assert!(
        max_coord_err(solve.nodes[1], [2.0, 0.0, 0.0]) < TOL,
        "free node 1 = {:?}, expected (2,0,0)",
        solve.nodes[1],
    );
}

// Member forces, force-density echo, and convergence flag on the case-(a)
// geometry. Each axial force is Nᵢ = qᵢ · Lᵢ where Lᵢ is the Euclidean
// length of member i measured on the *solved* coordinates (here all four
// cables are √1.25 long and q=1, so each force equals that length). The
// expected length is recomputed from the returned nodes so the assertion
// tracks the solve rather than a hard-coded constant.
#[test]
fn member_forces_are_q_times_solved_length_and_q_is_echoed() {
    let nodes = vec![
        [0.3, 0.2, 0.4], // free node 0 — off-solution
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
    ];
    let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
    let kinds = [MemberKind::Cable; 4];
    let q = [1.0, 1.0, 1.0, 1.0];
    let anchors = [1, 2, 3, 4];

    let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
        .expect("equal-q anchored cable net must be feasible");

    assert_eq!(
        solve.member_forces.len(),
        members.len(),
        "one axial force per member",
    );
    for (i, &(j, k)) in members.iter().enumerate() {
        let pj = solve.nodes[j];
        let pk = solve.nodes[k];
        let len =
            ((pj[0] - pk[0]).powi(2) + (pj[1] - pk[1]).powi(2) + (pj[2] - pk[2]).powi(2)).sqrt();
        let expected = q[i] * len;
        assert!(
            (solve.member_forces[i] - expected).abs() < TOL,
            "member_forces[{i}] = {}, expected q·L = {}",
            solve.member_forces[i],
            expected,
        );
    }

    // force_densities is an exact echo of the input q (a copy, not a
    // computed quantity), so exact equality must hold.
    assert_eq!(
        solve.force_densities,
        q.to_vec(),
        "force_densities must echo input q exactly",
    );

    assert!(
        solve.converged,
        "a well-posed anchored solve must report converged == true",
    );
}

// (b) A free node with no member path to any anchor leaves a zero row in the
// reduced stiffness D_ff → singular. The solve cannot recover that node.
#[test]
fn disconnected_free_node_is_singular_reduced_stiffness() {
    let nodes = vec![
        [0.0, 0.0, 0.0], // free node 0 — connected to the anchor
        [5.0, 0.0, 0.0], // free node 1 — floating: no members touch it
        [1.0, 0.0, 0.0], // anchor 2
    ];
    let members = [(0, 2)]; // only node 0 ↔ anchor; node 1 has no path
    let kinds = [MemberKind::Cable];
    let q = [1.0];
    let anchors = [2];

    assert_eq!(
        form_find_anchored(&nodes, &members, &kinds, &q, &anchors).unwrap_err(),
        FormFindError::SingularReducedStiffness,
    );
}

// (b2) TASK 7046 — a NUMERICALLY singular D_ff must be reported too,
// identically at every gauge, while the control (half the critical strut
// density) stays Ok. LU returns finite but huge (~1e13) coordinates, so
// the residual branch rejects, and structurally rather than by rounding
// luck: at that size f0's free terms `2·x0 − x1` lie on a dyadic grid G
// (their ulp), and A0.y = 1/3 has no finite binary expansion, so f0's
// y-row cannot balance to better than G/3 under any LU kernel.
// Measurements: task 7046.
#[test]
fn near_singular_reduced_stiffness_is_rejected_at_every_gauge() {
    // The strut is a rank-1 update of the cable-only D_ff, singular at
    // q_s = −1/(compliance between its ends) = −1/(2 + 2).
    const CRITICAL_STRUT_Q: f64 = -0.25;
    const TWO_POW_20: f64 = 1_048_576.0;
    let nodes = vec![
        [1.0, 0.0, 0.0],        // free f0
        [2.0, 0.0, 0.0],        // free f1
        [3.0, 0.0, 0.0],        // free f2
        [4.0, 0.0, 0.0],        // free f3
        [0.0, 1.0 / 3.0, -2.0], // anchor A0, y off every dyadic grid
        [5.0, -3.0, 4.0],       // anchor A1
    ];
    // A0 —cable— f0 —cable— f1 —STRUT— f2 —cable— f3 —cable— A1
    let members = [(1, 2), (4, 0), (0, 1), (2, 3), (3, 5)];
    let kinds = [
        MemberKind::Strut,
        MemberKind::Cable,
        MemberKind::Cable,
        MemberKind::Cable,
        MemberKind::Cable,
    ];
    let anchors = [4, 5];
    let solve_at = |strut_q: f64, lambda: f64| {
        let q = [strut_q * lambda, lambda, lambda, lambda, lambda];
        form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
    };

    for lambda in [1.0, TWO_POW_20, 1.0 / TWO_POW_20] {
        let near = solve_at(CRITICAL_STRUT_Q + 1e-14, lambda);
        assert_eq!(
            near.as_ref().err(),
            Some(&FormFindError::SingularReducedStiffness),
            "λ={lambda:e}: a near-critical strut leaves D_ff numerically singular, got {near:?}",
        );
        let control = solve_at(CRITICAL_STRUT_Q / 2.0, lambda);
        assert!(
            control.is_ok(),
            "λ={lambda:e}: the same chain at half the critical strut density is well posed, got {control:?}",
        );
    }
}

// (c) Anchoring every node leaves no free DOF to solve for.
#[test]
fn all_nodes_anchored_is_empty_free_set() {
    let nodes = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let members = [(0, 1)];
    let kinds = [MemberKind::Cable];
    let q = [1.0];
    let anchors = [0, 1]; // every node anchored → empty free set

    assert_eq!(
        form_find_anchored(&nodes, &members, &kinds, &q, &anchors).unwrap_err(),
        FormFindError::EmptyFreeSet,
    );
}

// ── γ (task 4414): surface-aware form_find_anchored_surfaces ───────────────

/// The shared tent golden, `reify_test_support::tensegrity_fixtures::TENT_*`,
/// in this module's [`MembraneCase`] shape — why it is shaped that way is
/// documented there.
fn tent_membrane() -> MembraneCase {
    let index = |i: i64| usize::try_from(i).expect("tent fixture indices are non-negative");
    let surfaces = TENT_TRIS
        .iter()
        .map(|&[i, j, k]| (index(i), index(j), index(k)))
        .collect();
    let anchors = TENT_ANCHORS.into_iter().map(index).collect();
    (TENT_NODE_COORDS.to_vec(), surfaces, anchors)
}

// (d) The pure-line path (empty surfaces) through the surface-aware entry
// must return exactly the landed form_find_anchored result, with an empty
// surface_stresses echo — the additive-extension invariant.
#[test]
fn surfaces_empty_matches_line_only_form_find_anchored() {
    let nodes = vec![
        [0.3, 0.2, 0.4],
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
    ];
    let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
    let kinds = [MemberKind::Cable; 4];
    let q = [1.0, 1.0, 1.0, 1.0];
    let anchors = [1, 2, 3, 4];

    let line = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
        .expect("line-only reference solve");
    let surf = form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &[], &[], &anchors)
        .expect("empty-surface path must match the line-only solve");

    assert!(surf.surface_stresses.is_empty(), "no surfaces ⇒ empty echo");
    assert_eq!(surf.converged, line.converged);
    assert_eq!(surf.force_densities, line.force_densities);
    assert_eq!(surf.member_forces.len(), line.member_forces.len());
    for (a, b) in surf.member_forces.iter().zip(line.member_forces.iter()) {
        assert!((a - b).abs() < 1e-12, "member force mismatch: {a} vs {b}");
    }
    assert_eq!(surf.nodes.len(), line.nodes.len());
    for (a, b) in surf.nodes.iter().zip(line.nodes.iter()) {
        assert!(
            max_coord_err(*a, *b) < 1e-12,
            "node mismatch: {a:?} vs {b:?}"
        );
    }
}

// (e) TASK 6119 — a free node touched by neither a member nor a triangle
// leaves its entire free-row block of D identically zero, so the net force
// on it is 0 for the vacuous reason that nothing acts on it at all. That
// must not be read as "converged": the honest outcome is
// SingularReducedStiffness (no path to any anchor), not echoing the
// caller's unsolved initial guess back as an "equilibrium".
#[test]
fn surfaces_solve_rejects_isolated_free_node_instead_of_echoing_it_back() {
    let nodes = vec![
        [0.0, 0.0, 0.0], // anchor
        [1.0, 0.0, 0.0], // anchor
        [0.0, 1.0, 0.0], // anchor
        [5.0, 5.0, 5.0], // free node 3 — isolated: no member, no triangle
    ];
    let surfaces = vec![(0, 1, 2)];
    let surface_stresses = vec![1.0];
    let members: Vec<(usize, usize)> = vec![];
    let kinds: Vec<MemberKind> = vec![];
    let q: Vec<f64> = vec![];
    let anchors = vec![0, 1, 2];

    assert_eq!(
        form_find_anchored_surfaces(
            &nodes,
            &members,
            &kinds,
            &q,
            &surfaces,
            &surface_stresses,
            &anchors
        )
        .unwrap_err(),
        FormFindError::SingularReducedStiffness,
    );
}

// (e2) TASK 6119 REVIEW — the degenerate-row guard exercised by (e) and
// `fd_criteria`'s unit tests must fire PER ROW: an aggregate guard on the
// MAX over free rows fires
// only when EVERY free row of D is zero, so a free node that IS
// connected, coexisting with an UNRELATED isolated free node, would hide
// the isolated one — which must still be rejected. Fixture: a
// flat symmetric tent — free node 0 sits at the exact centroid of 4
// anchors placed at (±1,0,0)/(0,±1,0), already at equilibrium by
// symmetry (the net cotangent force on node 0 is exactly 0) — plus an
// unrelated free node 5 touched by neither a member nor a triangle.
// REPRODUCED FIRST-HAND against the pre-fix (aggregate) guard: this
// returns `Ok(converged=true)` with `nodes[5] == [7.0, 7.0, 7.0]` — the
// caller's unsolved initial guess echoed straight back.
#[test]
fn surfaces_solve_rejects_isolated_free_node_alongside_a_connected_one() {
    let nodes = vec![
        [0.0, 0.0, 0.0],  // 0: free — connected, already at equilibrium
        [1.0, 0.0, 0.0],  // 1: anchor
        [0.0, 1.0, 0.0],  // 2: anchor
        [-1.0, 0.0, 0.0], // 3: anchor
        [0.0, -1.0, 0.0], // 4: anchor
        [7.0, 7.0, 7.0],  // 5: free — isolated: no member, no triangle
    ];
    let surfaces = vec![(0, 1, 2), (0, 2, 3), (0, 3, 4), (0, 4, 1)];
    let surface_stresses = vec![2.0; surfaces.len()];
    let members: Vec<(usize, usize)> = vec![];
    let kinds: Vec<MemberKind> = vec![];
    let q: Vec<f64> = vec![];
    let anchors = vec![1, 2, 3, 4];

    assert_eq!(
        form_find_anchored_surfaces(
            &nodes,
            &members,
            &kinds,
            &q,
            &surfaces,
            &surface_stresses,
            &anchors
        )
        .unwrap_err(),
        FormFindError::SingularReducedStiffness,
        "an isolated free node must reject even when another free node is connected",
    );
}

// ── ε (task 4416): form_find_anchored_surfaces_aniso verdicts + iso-equivalence ──

/// [`tent_membrane`] plus one isotropic-valued anisotropic prestress per
/// triangle, for the aniso solve tests.
#[allow(clippy::type_complexity)]
fn tent_aniso_fixture() -> (
    Vec<[f64; 3]>,
    Vec<(usize, usize, usize)>,
    Vec<AnisotropicSurfaceStress>,
    Vec<usize>,
) {
    let (nodes, surfaces, anchors) = tent_membrane();
    let sigma = 2.0;
    let prestress = vec![
        AnisotropicSurfaceStress {
            warp_dir: [1.0, 0.0, 0.0],
            sigma_warp: sigma,
            sigma_weft: sigma
        };
        surfaces.len()
    ];
    (nodes, surfaces, prestress, anchors)
}

// (e) All nodes anchored → EmptyFreeSet.
#[test]
fn aniso_solve_empty_free_set() {
    let (nodes, surfaces, prestress, _) = tent_aniso_fixture();
    let all_anchors: Vec<usize> = (0..nodes.len()).collect();
    let members: Vec<(usize, usize)> = vec![];
    let kinds: Vec<MemberKind> = vec![];
    let q: Vec<f64> = vec![];
    assert_eq!(
        form_find_anchored_surfaces_aniso(
            &nodes,
            &members,
            &kinds,
            &q,
            &surfaces,
            &prestress,
            &all_anchors
        )
        .unwrap_err(),
        AnisoFormFindError::EmptyFreeSet,
    );
}

// (f) ISOTROPIC-EQUIVALENCE: aniso solve with σ_w = σ_f = σ matches the
// isotropic solve to ≤ 1e-9 on node positions; converged == true.
#[test]
fn aniso_solve_isotropic_equivalence() {
    let (nodes, surfaces, prestress, anchors) = tent_aniso_fixture();
    let sigma = prestress[0].sigma_warp; // 2.0
    let sigmas = vec![sigma; surfaces.len()];
    let members: Vec<(usize, usize)> = vec![];
    let kinds: Vec<MemberKind> = vec![];
    let q: Vec<f64> = vec![];

    let aniso = form_find_anchored_surfaces_aniso(
        &nodes, &members, &kinds, &q, &surfaces, &prestress, &anchors,
    )
    .expect("σ_w=σ_f aniso solve must be feasible");

    let iso =
        form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &surfaces, &sigmas, &anchors)
            .expect("isotropic reference solve must be feasible");

    assert!(aniso.converged, "aniso solve must converge");
    assert_eq!(aniso.nodes.len(), iso.nodes.len());
    for (i, (a, b)) in aniso.nodes.iter().zip(iso.nodes.iter()).enumerate() {
        let err = max_coord_err(*a, *b);
        assert!(
            err < 1e-9,
            "node[{i}] aniso={a:?} iso={b:?} max_coord_err={err:e}",
        );
    }
}
