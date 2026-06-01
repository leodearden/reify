//! Integration golden for Tensegrity T1b — free-standing Force-Density
//! form-finding via the adaptive `GroupRatios` search.
//!
//! PRD reference: `docs/prds/v0_6/tensegrity-structures.md` Tier-1 leaf T1b.
//!
//! # The user-observable signal
//!
//! Given only the triplex *topology* (which members are struts vs cables) and
//! the *relative signs* of each member group's force density — struts
//! compressive, horizontals + verticals tensile — the free-standing kernel must
//! discover, with no anchors and no explicit `q`, the published self-stressed
//! triangular-antiprism (T-prism) equilibrium. This test drives
//! [`form_find_free`] through its public crate-root surface in
//! [`ForceDensitySpec::GroupRatios`] mode and asserts the gauge-invariant
//! goldens.
//!
//! # Fixture — the complete 9-cable triplex
//!
//! Six nodes (0,1,2 top; 3,4,5 bottom), 3 struts + 9 cables in struts-then-
//! cables member order:
//!   - struts        (0,4) (1,5) (2,3)
//!   - top horiz     (0,1) (1,2) (2,0)
//!   - bottom horiz  (3,4) (4,5) (5,3)
//!   - verticals     (0,3) (1,4) (2,5)
//!
//! Closed-form force densities (the equilibrium the search must reproduce):
//! struts `−√3`, the six horizontals `+1` (the reference / scale gauge),
//! verticals `+√3` — which make `D = CᵀQC` rank-deficient by exactly `d+1 = 4`
//! (eigenvalues `0,0,0,0,6,6`). The canonical metric realisation is a twisted
//! prism: equal-circumradius top/bottom triangles in parallel planes with a 30°
//! twist. See the kernel module docs and the `t1b-canonical-prism-geometry`
//! memory note for the derivation.

use reify_solver_elastic::{ForceDensitySpec, FreeFormResult, MemberKind, form_find_free};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// The complete 9-cable triplex topology in struts-then-cables member order,
/// together with the per-member kind tags.
fn triplex_topology() -> (Vec<(usize, usize)>, Vec<MemberKind>) {
    let members = vec![
        // struts
        (0, 4),
        (1, 5),
        (2, 3),
        // top horizontals
        (0, 1),
        (1, 2),
        (2, 0),
        // bottom horizontals
        (3, 4),
        (4, 5),
        (5, 3),
        // verticals
        (0, 3),
        (1, 4),
        (2, 5),
    ];
    let mut kinds = vec![MemberKind::Strut; 3];
    kinds.resize(members.len(), MemberKind::Cable);
    (members, kinds)
}

/// Per-member group ids parallel to the member order: struts → group 0, the six
/// horizontals (top + bottom) → group 1, verticals → group 2.
fn triplex_group_ids() -> Vec<usize> {
    vec![
        0, 0, 0, // struts
        1, 1, 1, // top horizontals
        1, 1, 1, // bottom horizontals
        2, 2, 2, // verticals
    ]
}

/// The canonical symmetric triplex prism (R = 1, height = 1, twist 30°), node
/// order matching `triplex_topology`: 0,1,2 top (z = 1) at azimuth 120°·i;
/// 3,4,5 bottom (z = 0) at azimuth 120°·i + 30°.
fn canonical_prism() -> Vec<[f64; 3]> {
    let deg = std::f64::consts::PI / 180.0;
    let top = |i: usize| {
        let a = 120.0 * (i as f64) * deg;
        [a.cos(), a.sin(), 1.0]
    };
    let bot = |i: usize| {
        let a = (120.0 * (i as f64) + 30.0) * deg;
        [a.cos(), a.sin(), 0.0]
    };
    vec![top(0), top(1), top(2), bot(0), bot(1), bot(2)]
}

/// A mildly perturbed (~1e-3 per coordinate, deterministic — no RNG) symmetric-
/// prism guess. Form-finding refines a guess, so the gauge-fixing aligns the
/// recovered shape to this near-symmetric placement; fixed offsets keep the
/// golden bit-stable.
fn perturbed_prism_guess() -> Vec<[f64; 3]> {
    const PERTURB: [[f64; 3]; 6] = [
        [0.0009, -0.0011, 0.0007],
        [-0.0013, 0.0006, 0.0010],
        [0.0012, 0.0008, -0.0009],
        [-0.0007, -0.0012, 0.0011],
        [0.0010, -0.0008, -0.0013],
        [-0.0011, 0.0013, 0.0006],
    ];
    canonical_prism()
        .iter()
        .zip(PERTURB.iter())
        .map(|(p, d)| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
        .collect()
}

// ---------------------------------------------------------------------------
// Small vector helpers (self-contained — integration tests cannot reach the
// kernel's `#[cfg(test)]` helpers).
// ---------------------------------------------------------------------------

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn member_len(nodes: &[[f64; 3]], m: (usize, usize)) -> f64 {
    norm(sub(nodes[m.0], nodes[m.1]))
}

/// Assert the members in `group` share one length within relative `tol`
/// (max−min ≤ tol·mean).
fn assert_equal_lengths(nodes: &[[f64; 3]], group: &[(usize, usize)], tol: f64, what: &str) {
    let lens: Vec<f64> = group.iter().map(|&m| member_len(nodes, m)).collect();
    let mean = lens.iter().sum::<f64>() / lens.len() as f64;
    let max = lens.iter().copied().fold(f64::MIN, f64::max);
    let min = lens.iter().copied().fold(f64::MAX, f64::min);
    assert!(
        (max - min) <= tol * mean,
        "{what} lengths must be equal within {tol} relative; got {lens:?} (mean {mean:.4})",
    );
}

// ---------------------------------------------------------------------------
// The T1b golden
// ---------------------------------------------------------------------------

#[test]
fn group_ratios_form_finds_published_triplex_prism_equilibrium() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_prism_guess();

    // Form-find from topology + relative signs only: struts compressive (seed
    // −1), horizontals (the reference/scale gauge) and verticals tensile
    // (seed +1), all magnitudes 1. The adaptive search discovers the relative
    // magnitudes.
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    let result: FreeFormResult = form_find_free(&guess, &members, &kinds, &spec)
        .expect("the triplex must form-find a free-standing equilibrium");

    // --- (1) Spectrum / convergence: a valid 3-D free-standing form has nullity
    // d+1 = 4. ---
    assert_eq!(result.nullity, 4, "form-found triplex must have nullity 4");
    assert!(result.converged, "T1b golden solve must converge");
    assert_eq!(result.nodes.len(), 6, "one recovered coordinate per node");

    // --- (2) Recovered relative force densities: struts −√3, verticals +√3,
    // horizontals pinned at the +1 reference. Every member in a group shares its
    // ratio. The search reaches the closed form to ~1e-9, so a 1e-6 tolerance is
    // comfortable. ---
    let s = 3.0_f64.sqrt();
    assert_eq!(result.force_densities.len(), members.len());
    for i in 0..3 {
        assert!(
            (result.force_densities[i] - (-s)).abs() < 1e-6,
            "strut {i} relative q must be ≈ −√3, got {}",
            result.force_densities[i],
        );
    }
    for i in 3..9 {
        assert!(
            (result.force_densities[i] - 1.0).abs() < 1e-12,
            "horizontal {i} is the reference group, must stay = 1, got {}",
            result.force_densities[i],
        );
    }
    for i in 9..12 {
        assert!(
            (result.force_densities[i] - s).abs() < 1e-6,
            "vertical {i} relative q must be ≈ +√3, got {}",
            result.force_densities[i],
        );
    }

    // --- (3) Metric prism shape (gauge-invariant goldens). 5% relative tol: a
    // correct recovery drifts only ~1e-3 from the symmetric prism (the in-null-
    // space part of the guess perturbation); a broken recovery spreads O(1). ---
    const MTOL: f64 = 5e-2;
    let nodes = &result.nodes;
    assert_equal_lengths(nodes, &members[0..3], MTOL, "strut");
    assert_equal_lengths(nodes, &members[3..9], MTOL, "horizontal cable");
    assert_equal_lengths(nodes, &members[9..12], MTOL, "vertical cable");

    // Top {0,1,2} and bottom {3,4,5} are each equilateral triangles...
    assert_equal_lengths(nodes, &[(0, 1), (1, 2), (2, 0)], MTOL, "top triangle edge");
    assert_equal_lengths(nodes, &[(3, 4), (4, 5), (5, 3)], MTOL, "bottom triangle edge");

    // ...lying in parallel planes (the two triangle normals are parallel).
    let n_top = cross(sub(nodes[1], nodes[0]), sub(nodes[2], nodes[0]));
    let n_bot = cross(sub(nodes[4], nodes[3]), sub(nodes[5], nodes[3]));
    let cos_planes = dot(n_top, n_bot).abs() / (norm(n_top) * norm(n_bot));
    assert!(
        cos_planes > 1.0 - 1e-3,
        "top/bottom triangle planes must be parallel; |cos| = {cos_planes:.6}",
    );

    // --- (4) Twist ≈ 30°: the rotation between the triangles about the prism
    // axis (centroid-to-centroid). Project a top node and its paired bottom node
    // onto the plane ⊥ axis and take their angle. ---
    let centroid = |g: &[usize]| {
        let mut c = [0.0; 3];
        for &i in g {
            for a in 0..3 {
                c[a] += nodes[i][a] / g.len() as f64;
            }
        }
        c
    };
    let c_top = centroid(&[0, 1, 2]);
    let c_bot = centroid(&[3, 4, 5]);
    let axis = {
        let a = sub(c_top, c_bot);
        let n = norm(a);
        [a[0] / n, a[1] / n, a[2] / n]
    };
    let proj = |p: [f64; 3], c: [f64; 3]| {
        let r = sub(p, c);
        let along = dot(r, axis);
        [
            r[0] - along * axis[0],
            r[1] - along * axis[1],
            r[2] - along * axis[2],
        ]
    };
    // Vertical pair (0,3): top node 0 and its bottom partner node 3.
    let u = proj(nodes[0], c_top);
    let w = proj(nodes[3], c_bot);
    let twist_deg = (dot(u, w) / (norm(u) * norm(w))).acos() * 180.0 / std::f64::consts::PI;
    assert!(
        (twist_deg - 30.0).abs() < 2.0,
        "vertical-pair twist must be ≈ 30°, got {twist_deg:.3}°",
    );

    // --- (5) Member forces N_i = q_i · L_i: struts compressive, cables tensile.
    // The sign of N_i follows q_i since every recovered length is positive. ---
    assert_eq!(result.member_forces.len(), members.len());
    for (idx, (&kind, &n_i)) in kinds.iter().zip(result.member_forces.iter()).enumerate() {
        match kind {
            MemberKind::Strut => assert!(
                n_i < 0.0,
                "strut {idx} must be compressive (N < 0), got {n_i}",
            ),
            MemberKind::Cable => assert!(
                n_i > 0.0,
                "cable {idx} must be tensile (N > 0), got {n_i}",
            ),
        }
    }
}
