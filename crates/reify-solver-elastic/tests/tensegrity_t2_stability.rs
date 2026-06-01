//! Integration golden for Tensegrity T2 — self-stress & prestress-stability
//! analysis kernel (null-space + tangent-stiffness).
//!
//! PRD reference: `docs/prds/v0_6/tensegrity-structures.md` §5 / Tier-2 leaf T2.
//!
//! # The user-observable signal
//!
//! Given a realised geometry, a member topology, and per-member force densities,
//! [`analyze_prestress_stability`] reports the classical self-stress / mechanism /
//! stability verdict (PRD §5's five fields). This test drives it through its
//! public crate-root surface on two opposite references:
//!
//!   * **Case 1 — canonical triplex.** The published twisted T-prism with the
//!     closed-form force densities is the textbook *super-stable* tensegrity: one
//!     self-stress state, one internal mechanism, Maxwell number `−6`, prestress-
//!     stable and super-stable.
//!   * **Case 2 — floppy planar square.** Four coplanar nodes joined by four edge
//!     cables (all `q = 1`) carry no self-stress (two perpendicular tensions
//!     cannot self-balance), so the framework is *not* prestress-stable and has at
//!     least one residual internal mechanism — the `s == 0` failure signal.
//!
//! # Fixtures
//!
//! Integration tests cannot reach the kernel's `#[cfg(test)]` helpers, so the
//! triplex + open-square fixtures are re-declared here verbatim (the established
//! T1a/T1b convention).

use reify_solver_elastic::{StabilityResult, analyze_prestress_stability};

// ---------------------------------------------------------------------------
// Fixtures (re-declared — integration tests cannot reach #[cfg(test)] helpers)
// ---------------------------------------------------------------------------

/// The complete 9-cable triplex topology in struts-then-cables member order
/// (3 struts + 9 cables). Mirrors the kernel's `triplex_members` fixture.
fn triplex_members() -> Vec<(usize, usize)> {
    vec![
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
    ]
}

/// The canonical symmetric triplex prism (R = 1, height = 1, twist 30°): nodes
/// 0,1,2 top (z = 1) at azimuth 120°·i; 3,4,5 bottom (z = 0) at 120°·i + 30°.
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

/// Closed-form force densities for the symmetric prism, struts-then-cables order:
/// struts −√3, the six horizontals +1, verticals +√3. These make `D` rank-
/// deficient by exactly 4 (D eigenvalues 0,0,0,0,6,6) — the super-stable spectrum.
fn closed_form_q() -> Vec<f64> {
    let s = 3.0_f64.sqrt();
    vec![
        -s, -s, -s, // struts
        1.0, 1.0, 1.0, // top horizontals
        1.0, 1.0, 1.0, // bottom horizontals
        s, s, s, // verticals
    ]
}

/// A planar open square: 4 coplanar nodes of the unit square (z = 0).
fn open_square() -> Vec<[f64; 3]> {
    vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ]
}

/// The four edge members of the open square, in ring order.
fn square_members() -> Vec<(usize, usize)> {
    vec![(0, 1), (1, 2), (2, 3), (3, 0)]
}

// ---------------------------------------------------------------------------
// Case 1 — canonical triplex: the PRD §5 super-stable golden
// ---------------------------------------------------------------------------

#[test]
fn canonical_triplex_is_super_stable() {
    let result = analyze_prestress_stability(&canonical_prism(), &triplex_members(), &closed_form_q())
        .expect("the canonical triplex + closed-form q is a well-formed analysis input");

    // The full five-field PRD §5 verdict, pinned exactly: one self-stress state,
    // one internal mechanism, Maxwell number m − d·N = 12 − 18 = −6, prestress-
    // stable, and super-stable (D PSD with rank N−d−1 = 2).
    assert_eq!(
        result,
        StabilityResult {
            self_stress_states: 1,
            mechanisms: 1,
            maxwell: -6,
            stable: true,
            super_stable: true,
        },
    );
}

// ---------------------------------------------------------------------------
// Case 2 — floppy planar square: the s == 0 not-stable signal
// ---------------------------------------------------------------------------

#[test]
fn floppy_planar_square_is_not_stable() {
    let q = vec![1.0_f64; 4];
    let result = analyze_prestress_stability(&open_square(), &square_members(), &q)
        .expect("the open square + unit cable q is a well-formed analysis input");

    // Four independent edge directions ⇒ rank(A) = 4 = m ⇒ no self-stress.
    assert_eq!(
        result.self_stress_states, 0,
        "a planar edge-cabled square carries no self-stress",
    );
    // nullity(Aᵀ) = 8 minus the 6 rigid-body modes ⇒ residual internal
    // mechanisms remain (2 for the square); at least one is the floppy signal.
    assert!(
        result.mechanisms >= 1,
        "the floppy square retains at least one internal mechanism, got {}",
        result.mechanisms,
    );
    // No self-stress ⇒ nothing to prestress-stabilise ⇒ not stable, and the
    // rank(D) ≠ N−d−1 condition fails ⇒ not super-stable.
    assert!(!result.stable, "no self-stress ⇒ not prestress-stable");
    assert!(!result.super_stable, "no self-stress ⇒ not super-stable");
}
