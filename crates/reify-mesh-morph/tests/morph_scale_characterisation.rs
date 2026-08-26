//! Characterisation harness: gmsh from-scratch tetrahedralisation wall-clock
//! vs. elasticity-morph wall-clock, at the 10K and 100K element scales.
//!
//! ## Background
//!
//! Task #2953 ("end-to-end slider-responsiveness benchmark", PRD
//! `docs/prds/v0_3/mesh-morphing.md:139`) asserts a >=10x wall-clock
//! reduction for morph-vs-always-remesh at the 100K scale. That threshold
//! has no measurement basis in this repo: nothing in `reify-mesh-morph`
//! reads a clock, and the only number the PRD offers is the design-time
//! estimate at `docs/prds/v0_3/mesh-morphing.md:11` — "at 100K elements,
//! that's ~3s serial / ~0.3s parallel per tick of mesh time" — which
//! carries no host, no fixture, and no provenance.
//!
//! This binary supplies that basis: it measures both arms on the same
//! bracket geometry at the same two scales on one host and prints the
//! achieved counts alongside the achieved times, so a threshold can be
//! derived from a measurement rather than from an estimate.
//!
//! It is a harness, not a gate. Nothing here asserts a performance
//! property, and nothing here should ever become a CI-blocking bound
//! without repetition and statistics it deliberately does not collect.
//!
//! ## Tests in this file
//!
//! Always-on (cheap; every one guards a helper the harness depends on, and
//! all of them stay inside the existing `calibration.rs` cost envelope):
//!
//! - [`bracket_fixture_reaches_the_10k_and_100k_tet_calibration_scales`] —
//!   pure fixture generation; pins the two scale bands.
//! - [`bracket_boundary_surface_is_closed_outward_wound_and_fully_referenced`] —
//!   the tet -> boundary-surface extractor that feeds the gmsh arm.
//! - [`morph_once_times_a_connectivity_preserving_fillet_perturbation`] —
//!   the timed morph helper, at n=4.
//! - [`gmsh_tetrahedralise_produces_tets_at_a_requested_mesh_size`] — the
//!   timed gmsh helper, coarse; `#[cfg(has_gmsh)]`. Its
//!   `#[cfg(not(has_gmsh))]` sibling
//!   [`gmsh_arm_is_absent_in_a_stub_build`] keeps a stub build honest.
//! - [`nearest_by_tet_count_selects_the_closest_rung`] — the pure
//!   count-matching function that joins the two ladders.
//!
//! Ignored (the driver): [`gmsh_from_scratch_vs_morph_wall_clock_at_10k_and_100k`]
//! composes those helpers, prints every measurement, and asserts nothing.
//!
//! ## How to run
//!
//! ```text
//! cargo test -p reify-mesh-morph --test morph_scale_characterisation -- --ignored --nocapture --test-threads=1
//! ```
//!
//! ## What these numbers do and do not characterise
//!
//! The morph arm is forced serial: `src/elasticity.rs` hardcodes
//! `AssemblyMode::Deterministic` and `SolverMode::Deterministic`, and
//! `elasticity_morph` exposes no assembly/solve split — so the harness
//! measures the combined call on the serial path only, and cannot
//! attribute time between assembly and CG. The gmsh arm is likewise
//! forced single-threaded (`MeshingOptions::deterministic = true`), which
//! is the apples-to-apples counterpart. Neither arm says anything about
//! the parallel path either PRD figure also quotes.

#[path = "calibration/fixtures.rs"]
mod fixtures;

// ── Shared geometry ──────────────────────────────────────────────────────────
//
// One bracket, swept only in `fillet_radius`. Connectivity is invariant under
// that parameter (only the inner fillet-arc vertices move), which is what
// makes the identity surface correspondence the morph arm uses legal.

/// Full extent of each bracket arm along its long axis.
const ARM_LENGTH: f64 = 1.0;

/// Uniform bracket thickness and extrusion depth.
const THICKNESS: f64 = 0.2;

/// Source fillet radius — the geometry both arms start from.
const FILLET_BASE: f64 = 0.05;

/// Target fillet radius for the morph arm. A small perturbation
/// (+0.01, i.e. +20 % of `FILLET_BASE`), connectivity-preserving and well
/// inside the solver's operating range — the same step-size scale
/// `tests/calibration.rs` sweeps over.
const FILLET_TARGET: f64 = 0.06;

// ── Scale constants ──────────────────────────────────────────────────────────
//
// The `bracket` generator's P1 element count is closed-form for n >= 2:
//
//     tets(n) = 18n^3 + 12n^2 - 6n
//
// (6 tets per hex over the polar zone's n_z*n_a*n_r cells, plus the two arm
// zones, each contributing 6 tets per hex over n_z*n_arm*(n_r+1) cells less
// the excluded corner column, plus one 3-tet wedge bridge per z-layer per
// arm.) Both scale constants below are read off that formula rather than
// guessed, and `bracket_fixture_reaches_the_10k_and_100k_tet_calibration_scales`
// pins the result empirically.

/// Resolution reaching the ~10K element band: `tets(8) = 9,936`.
const N_10K: usize = 8;

/// Resolution reaching the ~100K element band: `tets(18) = 108,756`.
/// (`tets(17) = 91,800` is the rung below, which is why the test's band is
/// wide enough to admit either.)
const N_100K: usize = 18;


/// The `bracket` fixture must actually reach both calibration bands this
/// whole harness rests on: ~10K tets and ~100K tets, at P1 order, with a
/// usable surface-node index vector at each scale.
///
/// Cheap and always-on: pure fixture generation (integer arithmetic +
/// vertex emission), no morph and no meshing.
///
/// Bands, not exact equality. The generator's closed-form P1 element count
/// is `tets(n) = 18n^3 + 12n^2 - 6n` for n >= 2, giving 9,936 at n=8 and
/// 108,756 at n=18. The 100K band deliberately also admits n=17's 91,800
/// so an off-by-one in the chosen n cannot doom the test.
#[test]
fn bracket_fixture_reaches_the_10k_and_100k_tet_calibration_scales() {
    use reify_ir::ElementOrderTag;

    for (n, lo, hi) in [
        (N_10K, 9_000usize, 11_000usize),
        (N_100K, 90_000, 130_000),
    ] {
        let (mesh, surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, n);

        assert_eq!(
            mesh.element_order(),
            Some(ElementOrderTag::P1),
            "bracket(n={n}) must be a P1 tet mesh — the elasticity morph and the \
             gmsh arm are both P1-only"
        );

        let tets = mesh
            .tet_indices()
            .unwrap_or_else(|| panic!("bracket(n={n}) must expose tet connectivity"))
            .len()
            / 4;
        assert!(
            (lo..=hi).contains(&tets),
            "bracket(n={n}) produced {tets} tets, outside the calibration band \
             {lo}..={hi}; closed form 18n^3 + 12n^2 - 6n predicts {}",
            18 * n * n * n + 12 * n * n - 6 * n
        );

        assert!(
            !surface_indices.is_empty(),
            "bracket(n={n}) returned an empty surface-node index vector; the morph \
             arm has no Dirichlet data without it"
        );
        let n_vertices = mesh.vertices.len() / 3;
        for &i in &surface_indices {
            assert!(
                (i as usize) < n_vertices,
                "bracket(n={n}) surface index {i} is out of range for {n_vertices} vertices"
            );
        }
    }
}
