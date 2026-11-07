//! Shared fixtures for the Tensegrity combined (struts+cables+membrane)
//! free-standing golden family: the triplex topology, canonical symmetric
//! prism, perturbed-guess generator, and the closed-form combined
//! self-stress `analytic_combined_q`.
//!
//! Not a test binary: cargo auto-discovers only top-level `tests/*.rs`, so
//! this `tests/common/mod.rs` is compiled *into* each consumer rather than
//! run as one itself.
//!
//! Currently imported by `tensegrity_free_surface_gauge_covariance.rs` only.
//! `tensegrity_delta_combined_form_find.rs` still carries its own copy of
//! these same helpers — task 6413 holds a lock on
//! `tensegrity_free_surface_gauge_covariance.rs` only, not that file, so
//! migrating it to import from here instead is left as a follow-up.

use reify_solver_elastic::MemberKind;

/// The complete triplex (3 struts + 9 cables).
pub fn triplex_topology() -> (Vec<(usize, usize)>, Vec<MemberKind>) {
    let members = vec![
        (0, 4), (1, 5), (2, 3), // struts
        (0, 1), (1, 2), (2, 0), // top horizontals
        (3, 4), (4, 5), (5, 3), // bottom horizontals
        (0, 3), (1, 4), (2, 5), // verticals
    ];
    let kinds = vec![
        MemberKind::Strut, MemberKind::Strut, MemberKind::Strut,
        MemberKind::Cable, MemberKind::Cable, MemberKind::Cable,
        MemberKind::Cable, MemberKind::Cable, MemberKind::Cable,
        MemberKind::Cable, MemberKind::Cable, MemberKind::Cable,
    ];
    (members, kinds)
}

pub fn triplex_group_ids() -> Vec<usize> {
    vec![0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2]
}

/// Canonical symmetric T-prism.
pub fn canonical_prism() -> Vec<[f64; 3]> {
    let s = 3.0_f64.sqrt() / 2.0;
    let h = 1.0_f64;
    vec![
        [1.0, 0.0, h], [-0.5, s, h], [-0.5, -s, h],
        [-0.5, -s, -h], [1.0, 0.0, -h], [-0.5, s, -h],
    ]
}

/// Perturbed prism guess, with the fixed `PERTURB` displacement table scaled
/// by `k` — lets a grid probe how far off-symmetry a starting guess can be
/// while still converging. `perturbed_prism_guess()` delegates here at
/// `k = 1.0`.
pub fn perturbed_prism_guess_scaled(k: f64) -> Vec<[f64; 3]> {
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
        .map(|(p, d)| [p[0] + k * d[0], p[1] + k * d[1], p[2] + k * d[2]])
        .collect()
}

pub fn perturbed_prism_guess() -> Vec<[f64; 3]> {
    perturbed_prism_guess_scaled(1.0)
}

/// Top {0,1,2} and bottom {3,4,5} membrane triangles.
pub fn prism_surfaces() -> Vec<(usize, usize, usize)> {
    vec![(0, 1, 2), (3, 4, 5)]
}

/// Closed-form COMBINED self-stress for the triplex + two equilateral
/// membrane triangles: every cotangent in the surface stencil is
/// cot(60°) = 1/√3 at the symmetric realisation, so `Σ_T σ_T·L_T` collapses
/// to a uniform extra edge weight `w = σ·cot(60°)/2 = σ/(2√3)` on the six
/// horizontal cables, giving `q_strut = -(√3 + σ/2)`, `q_horiz = 1`,
/// `q_vert = +(√3 + σ/2)`.
pub fn analytic_combined_q(sigma: f64) -> Vec<f64> {
    let a = 3.0_f64.sqrt() + sigma / 2.0;
    vec![
        -a, -a, -a, // struts
        1.0, 1.0, 1.0, // top horizontals
        1.0, 1.0, 1.0, // bottom horizontals
        a, a, a, // verticals
    ]
}
