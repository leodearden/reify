//! Characterisation harness: gmsh from-scratch tetrahedralisation wall-clock
//! vs. elasticity-morph wall-clock, at the 10K and 100K element scales.
//!
//! ## Purpose
//!
//! Task #2953 ("end-to-end slider-responsiveness benchmark", PRD
//! `docs/prds/v0_3/mesh-morphing.md:139`) asserts a >=10x wall-clock
//! reduction for morph-vs-always-remesh at the 100K scale, and the PRD's own
//! design-time estimate at `docs/prds/v0_3/mesh-morphing.md:11` puts a tick
//! of 100K mesh time at "~3s serial / ~0.3s parallel". Neither figure had a
//! measurement basis in this repo: nothing in `reify-mesh-morph` read a
//! clock. This binary supplies one, measuring both arms on the same bracket
//! geometry at the same two scales on one host and printing the achieved
//! counts alongside the achieved times, so a threshold can be derived from a
//! measurement rather than from an estimate.
//!
//! It is a harness, not a gate. Nothing here asserts a performance property,
//! and nothing here should become a CI-blocking bound without repetition and
//! the statistics it deliberately does not collect.
//!
//! ## What it measured
//!
//! **`docs/notes/morph-vs-remesh-scale-characterisation.md`** — the
//! authoritative record: both profiles' tables, the #2953 analysis, the
//! scaling breakdown and the standing caveats. The numbers live there rather
//! than here so that a measurement log cannot drift against the code it is
//! embedded in.
//!
//! The outcome, qualitatively, so a reader learns it without the hop: at the
//! 100K scale the morph is **more than an order of magnitude SLOWER** than the
//! from-scratch remesh, so #2953's premise is inverted rather than merely
//! unmet; at 10K the two arms are roughly par, with which one wins depending
//! on the build profile. Every figure behind those two sentences is in the
//! note and deliberately nowhere else — a number repeated here is a number
//! that can go stale here.
//!
//! ## How to run
//!
//! ```text
//! cargo test -p reify-mesh-morph --test morph_scale_characterisation -- --ignored --nocapture --test-threads=1
//! ```
//!
//! ## Tests in this file
//!
//! Always-on (cheap; every one guards a helper the harness depends on, and
//! all of them stay inside the existing `calibration.rs` cost envelope):
//!
//! - [`bracket_fixture_reaches_the_10k_and_100k_tet_calibration_scales`] —
//!   pure fixture generation; pins the two scale bands.
//! - [`bracket_boundary_surface_is_closed_outward_wound_and_fully_referenced`] —
//!   the tet -> boundary-surface extractor that feeds the gmsh arm, swept
//!   over every resolution the driver uses.
//! - [`morph_once_times_a_connectivity_preserving_fillet_perturbation`] —
//!   the timed morph helper, at n=4.
//! - [`gmsh_tetrahedralise_produces_tets_at_a_requested_mesh_size`] — the
//!   timed gmsh helper, coarse; `#[cfg(has_gmsh)]`. Its
//!   `#[cfg(not(has_gmsh))]` sibling
//!   [`gmsh_arm_is_absent_in_a_stub_build`] keeps a stub build honest.
//! - [`nearest_by_tet_count_selects_the_closest_rung`] — the pure
//!   count-matching function that joins the two ladders. Makes no gmsh call
//!   (it is driven by synthetic values), but carries `#[cfg(has_gmsh)]`
//!   because the `GmshMeasurement` it selects over does: a stub build has no
//!   ladder to join.
//!
//! Ignored (the driver): [`gmsh_from_scratch_vs_morph_wall_clock_at_10k_and_100k`]
//! composes those helpers, prints every measurement, and asserts nothing.
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
//!
//! Each pairing also carries a residual count mismatch: the two ladders are
//! swept independently (morph by resolution `n`, gmsh by `mesh_size`) and
//! never land on the same tet count, so a ratio is only as meaningful as its
//! mismatch is small. The driver prints that mismatch next to every ratio.

#[path = "calibration/boundary.rs"]
mod boundary;
#[path = "calibration/fixtures.rs"]
mod fixtures;

use std::time::{Duration, Instant};

#[cfg(has_gmsh)]
use reify_ir::GeometryError;
use reify_ir::VolumeMesh;
use reify_mesh_morph::{ElasticityFailure, MorphOptions, elasticity_morph};

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

    for (n, lo, hi) in [(N_10K, 9_000usize, 11_000usize), (N_100K, 90_000, 130_000)] {
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

/// The tet -> boundary-surface extractor that feeds the gmsh arm must emit
/// exactly the once-occurring tet faces, wound outward, compacted onto only
/// the vertices it actually references — at every resolution the harness
/// itself uses, not merely at the cheap one.
///
/// The primary assertion target is `Mesh::validate` (in
/// `crates/reify-ir/src/geometry.rs`; cited by symbol rather than by line,
/// which goes stale silently in a 4000-line file): it is
/// precisely the producer-obligation set gmsh's preflight demands — finite,
/// index-valid, non-degenerate, closed, and consistently wound on the
/// position-welded quotient.
///
/// ## Why the resolution sweep
///
/// Block-interface conformity is the property `fixtures::bracket`'s polar
/// half-turn ordering and arm-2 wedge ordering exist to guarantee, and the
/// driver treats its violation as catastrophic rather than local: a single
/// open surface makes HXT's `mesh_generate` fail, and per
/// [`gmsh_tetrahedralise`]'s own notes that failure leaves thread-local
/// state which survives `gmshClear()` and silently zeroes every SUBSEQUENT
/// rung of the ladder. Pinning conformity only at n=4 while the driver
/// extracts and meshes surfaces at [`N_10K`] and [`N_100K`] would leave that
/// blast radius uncovered for the two resolutions that actually run.
///
/// The sweep runs n = 1, 2, 3, 4, [`N_10K`], [`N_100K`]: the coarse end
/// because n=1 is the one resolution not covered by the generator's `n >= 2`
/// closed form, n=4 because its surface is pinned to recorded literals below,
/// and the last two because they are what the driver meshes.
///
/// Cheap and always-on even so: the whole sweep is pure fixture generation
/// plus extraction, no morph and no meshing, and stays inside the existing
/// `calibration.rs` cost envelope (~1 s at the time of writing).
#[test]
fn bracket_boundary_surface_is_closed_outward_wound_and_fully_referenced() {
    use std::collections::{HashMap, HashSet};

    for n in [1, 2, 3, 4, N_10K, N_100K] {
        let (mesh, _surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, n);
        let surface = boundary::boundary_surface(&mesh);

        // (1) The full mesh contract. Naming the Err variant means a contract
        // violation reports which obligation failed rather than "assert failed".
        if let Err(violation) = surface.validate(1e-6) {
            panic!(
                "boundary_surface(bracket(n={n})) violates the mesh contract, so \
                 gmsh's preflight would reject it: {violation:?}"
            );
        }

        assert_eq!(
            surface.indices.len() % 3,
            0,
            "boundary_surface(n={n}) must emit whole triangles; got {} indices",
            surface.indices.len()
        );

        let tris = surface.indices.len() / 3;
        let verts = surface.vertices.len() / 3;

        // (2) Euler characteristic of a closed genus-0 manifold: every edge at
        // degree exactly 2, and V - E + F = 2. This is deliberately NOT a
        // recomputation of the extractor's own algorithm — re-deriving the
        // once-occurring-face set here from `tet_indices` would share the
        // keying with the implementation, so any defect common to both (a
        // per-tet rather than per-face count, a changed face enumeration)
        // would satisfy the assertion identically. Edge degree and the Euler
        // sum are properties of the EMITTED surface alone, and they are what
        // "watertight enough for HXT" actually means.
        //
        // The count is taken in index space, not on the position-welded
        // quotient `validate` uses, which makes it the stronger statement:
        // two coincident-but-distinct vertices at a block interface weld away
        // under `validate` but break degree-2 here. That is exactly the
        // non-conformity this sweep is looking for.
        let mut edge_degree: HashMap<[u32; 2], usize> = HashMap::new();
        for tri in surface.indices.chunks_exact(3) {
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let mut edge = [a, b];
                edge.sort_unstable();
                *edge_degree.entry(edge).or_insert(0) += 1;
            }
        }
        let irregular = edge_degree.values().filter(|&&d| d != 2).count();
        assert_eq!(
            irregular, 0,
            "boundary_surface(n={n}) emitted {irregular} edges of degree != 2 out \
             of {}; the surface is not a closed manifold, and gmsh's HXT backend \
             fails on an open input in a way that poisons every later rung",
            edge_degree.len()
        );
        let edges = edge_degree.len();
        assert_eq!(
            verts as i64 - edges as i64 + tris as i64,
            2,
            "boundary_surface(n={n}) has V-E+F = {}-{}+{} != 2; a closed genus-0 \
             surface is what the bracket's three aligned blocks are supposed to \
             bound",
            verts,
            edges,
            tris
        );

        // (2b) At n=4 the three counts are pinned to the figures recorded
        // independently in `fixtures::bracket`'s "## Conformity" paragraph, so
        // the docstring and the extractor cannot drift apart unnoticed.
        if n == 4 {
            assert_eq!(
                (verts, edges, tris),
                (248, 738, 492),
                "boundary_surface(bracket(n=4)) must reproduce the surface recorded \
                 in `fixtures::bracket`'s conformity paragraph (248 vertices / 738 \
                 edges / 492 triangles); got {verts} / {edges} / {tris}"
            );
        }

        // (3) Compaction: every emitted vertex is referenced by some triangle,
        // i.e. the extractor remaps rather than carrying interior nodes through.
        let used: HashSet<u32> = surface.indices.iter().copied().collect();
        assert_eq!(
            used.len(),
            verts,
            "boundary_surface(n={n}) must carry only referenced vertices; {verts} \
             emitted but only {} referenced (interior nodes would be handed to \
             gmsh for nothing)",
            used.len()
        );

        // (4) Compaction is a strict narrowing of the volume mesh's vertex table.
        assert!(
            verts <= mesh.vertices.len() / 3,
            "boundary_surface(n={n}) emitted {verts} vertices from a {}-vertex \
             volume mesh",
            mesh.vertices.len() / 3
        );

        // (5) Winding SIGN — the half of "outward" that `validate` structurally
        // cannot see. Its Closed/ConsistentWinding obligation asks only that
        // every directed edge on the position-welded quotient have its reverse
        // exactly once: that is orientABILITY, and a globally INVERTED closed
        // surface satisfies it just as happily as an outward one. Nothing
        // downstream catches the difference either — gmsh meshes the inverted
        // surface without complaint — so `orient_outward` could silently flip
        // and every other assertion in this file would stay green.
        //
        // The divergence theorem pins the sign in O(tris) with no new
        // dependency: summing the signed volume of the tetrahedron each triangle
        // spans with the origin, `dot(v0, cross(v1, v2)) / 6`, totals +V for an
        // outward-wound closed surface and -V for an inward-wound one.
        let vertex = |i: u32| -> [f64; 3] {
            let base = i as usize * 3;
            [
                surface.vertices[base] as f64,
                surface.vertices[base + 1] as f64,
                surface.vertices[base + 2] as f64,
            ]
        };
        let mut signed_volume = 0.0_f64;
        for tri in surface.indices.chunks_exact(3) {
            let (a, b, c) = (vertex(tri[0]), vertex(tri[1]), vertex(tri[2]));
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            signed_volume += (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) / 6.0;
        }
        assert!(
            signed_volume > 0.0,
            "boundary_surface(n={n}) emitted an INWARD-wound surface (signed volume \
             {signed_volume}); `Mesh::validate` cannot see this, so this assertion \
             is the only thing standing between a sign flip in `orient_outward` \
             and a silently inverted gmsh input"
        );

        // ...and loosely, the right magnitude. The bracket's analytic volume is
        //
        //     (2*L*T - T^2 - (r^2 - pi*r^2/4)) * T
        //   = (2*1.0*0.2 - 0.2^2 - (0.05^2 - pi*0.05^2/4)) * 0.2
        //   = 0.071893
        //
        // for a smooth fillet; the fixture facets that concave arc and so
        // undershoots a little (0.069617 measured at n=4). A +/-15% band absorbs
        // faceting at any resolution while still catching a structural defect the
        // sign check alone would miss — a duplicated or dropped face set moves the
        // total by a factor, not by a few percent.
        const ANALYTIC_VOLUME: f64 = 0.071_893;
        assert!(
            (0.85 * ANALYTIC_VOLUME..=1.15 * ANALYTIC_VOLUME).contains(&signed_volume),
            "boundary_surface(n={n}) enclosed {signed_volume}, outside +/-15% of \
             the bracket's analytic volume {ANALYTIC_VOLUME} — the surface is \
             closed and outward-wound but does not bound the solid it came from"
        );
    }
}

// ── Morph arm ────────────────────────────────────────────────────────────────

/// One timed `elasticity_morph` call, reported as data rather than as a
/// pass/fail.
///
/// `result` is carried unconverted on purpose. At the 100K scale the serial
/// Jacobi-CG (`max_iter` 1000, tol 1e-8, 61,617 DOF at n=18) may legitimately
/// return [`ElasticityFailure::SolverNotConverged`], and the time taken to reach
/// `max_iter` is itself characterisation data — arguably the headline
/// finding, since a non-converging serial CG makes the morph arm *slower*
/// than a remesh rather than faster. Unwrapping here would destroy the run
/// that produced the most interesting number in it.
#[derive(Debug)]
struct MorphMeasurement {
    /// The `bracket` resolution this measurement was taken at.
    n: usize,
    /// P1 tet count of the source mesh (equal to the morphed mesh's, since
    /// the morph is a node-position update).
    tets: usize,
    /// Node count of the source mesh. The solve's displacement DOF count is
    /// `3 * nodes` and is derived at the one place it is printed rather than
    /// stored, so there is no second copy to fall out of step.
    nodes: usize,
    /// Wall-clock of the `elasticity_morph` call ALONE — see [`morph_once`]
    /// for what is deliberately excluded.
    elapsed: Duration,
    /// The morph's own result, uninterpreted.
    result: Result<VolumeMesh, ElasticityFailure>,
}

/// Time a single connectivity-preserving fillet perturbation
/// (`FILLET_BASE` -> `FILLET_TARGET`) on `bracket` at resolution `n`.
///
/// ## What the timed region covers
///
/// Only the `elasticity_morph` call. Both fixture constructions and the
/// prescribed-position build stay outside it: they are procedural mesh
/// generation, not morph work, and at n=18 they are far from negligible —
/// folding them in would inflate the morph arm against the gmsh arm and
/// bias the very ratio this harness exists to report.
///
/// ## Why not `sweep::run_sweep`
///
/// `tests/calibration/sweep.rs::run_sweep` drives the identical
/// correspondence, but wraps the morph in three additional `quality_check`
/// passes and `unwrap`s the result. Both are disqualifying here: the first
/// contaminates the wall-clock, the second turns a `SolverNotConverged` at
/// 100K into a panic that would lose the whole run. The correspondence
/// *pattern* is reused; the function deliberately is not.
///
/// The identity surface correspondence is legal because `bracket`'s
/// connectivity is invariant under `fillet_radius` — only the inner
/// fillet-arc vertices move — so the source's surface-index list indexes
/// the target's vertex table as well.
fn morph_once(n: usize) -> MorphMeasurement {
    let (source, surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, n);
    let (target, _target_surface_indices) =
        fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_TARGET, n);

    let prescribed_positions: Vec<(u32, [f64; 3])> = surface_indices
        .iter()
        .map(|&i| {
            let pos = target.vertex_f64(i).unwrap_or_else(|| {
                panic!(
                    "morph_once(n={n}): surface index {i} is out of range for the \
                     target mesh ({} vertices) — the two fixture evaluations \
                     disagree on connectivity, which would invalidate the entire \
                     comparison rather than merely perturb it",
                    target.vertices.len() / 3
                )
            });
            (i, pos)
        })
        .collect();

    let tets = source.tet_indices().map_or(0, |t| t.len() / 4);
    let nodes = source.vertices.len() / 3;

    let started = Instant::now();
    let result = elasticity_morph(&source, &prescribed_positions, &MorphOptions::default());
    let elapsed = started.elapsed();

    MorphMeasurement {
        n,
        tets,
        nodes,
        elapsed,
        result,
    }
}

/// The timed morph helper must run a real, connectivity-preserving morph and
/// must actually read the clock.
///
/// Deliberately at n=4 — the cost point `tests/calibration.rs` already pays.
/// The 10K and 100K legs belong to the `#[ignore]`d driver, not to the
/// always-on suite; this test guards the helper's contract, not its speed.
///
/// Connectivity preservation is the load-bearing property here: it is what
/// makes the morph arm comparable to the from-scratch remesh arm at all. A
/// morph that changed the tet table would be solving a different problem
/// than the one whose wall-clock this harness reports.
#[test]
fn morph_once_times_a_connectivity_preserving_fillet_perturbation() {
    use std::time::Duration;

    let measurement = morph_once(4);

    // Regenerate the source independently rather than reading it back off the
    // measurement, so the assertions below compare against the fixture rather
    // than against whatever `morph_once` happened to build.
    let (source, surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, 4);
    let source_tet_indices = source
        .tet_indices()
        .expect("bracket(n=4) must expose tet connectivity")
        .len();

    let morphed = match &measurement.result {
        Ok(morphed) => morphed,
        Err(failure) => panic!(
            "morph_once(4) must succeed: a +0.01 fillet perturbation at n=4 is \
             well inside the solver's operating range, so a failure here is a \
             rig bug, not a tuning concern — got {failure:?}"
        ),
    };

    assert_eq!(
        morphed
            .tet_indices()
            .expect("the morphed mesh must expose tet connectivity")
            .len(),
        source_tet_indices,
        "the morph must preserve connectivity — it is a node-position update, \
         and that is what makes its wall-clock comparable to a remesh's"
    );

    // Self-consistency of the reported shape: these three fields are what the
    // driver prints and what any downstream ratio is normalised by, so a
    // mislabelled scale would silently corrupt every conclusion drawn from
    // the table.
    assert_eq!(
        measurement.n, 4,
        "the measurement must report its own scale"
    );
    assert_eq!(
        measurement.tets,
        source_tet_indices / 4,
        "reported tet count must match the fixture"
    );
    assert_eq!(
        measurement.nodes,
        source.vertices.len() / 3,
        "reported node count must match the fixture"
    );
    assert!(
        measurement.elapsed > Duration::ZERO,
        "the clock was never read — `elapsed` is {:?}",
        measurement.elapsed
    );

    // Everything above is satisfied by a morph that returns its input
    // VERBATIM: index-length equality, three self-consistency fields read off
    // the same fixture, and a nonzero clock. That is the one regression this
    // harness could not survive — an identity `elasticity_morph` would make
    // the whole 10K/100K table a timing of a no-op, and every ratio derived
    // from it meaningless. So pin the two properties that make the timed call
    // the morph this file claims to be measuring.

    // (a) The mesh actually moved.
    assert_eq!(
        morphed.vertices.len(),
        source.vertices.len(),
        "the morph must preserve the vertex table's shape"
    );
    let max_delta = source
        .vertices
        .iter()
        .zip(&morphed.vertices)
        .map(|(a, b)| (*a as f64 - *b as f64).abs())
        .fold(0.0_f64, f64::max);
    assert!(
        max_delta > 0.0,
        "the morph returned the source mesh unmoved (max coordinate delta is \
         exactly 0) — an identity morph satisfies every other assertion in \
         this test while making the harness time a no-op"
    );

    // (b) The prescribed nodes landed where they were prescribed. This is the
    // stronger half: it rules out a morph that moves the mesh but ignores the
    // Dirichlet data. `elasticity_morph` applies the BCs by row elimination
    // and writes `old + u` back narrowed to f32, so a satisfied prescribed
    // node reproduces the target fixture's own f32 coordinate to within f32
    // rounding plus the CG residual — orders of magnitude below the 0.01
    // fillet step being prescribed, so the tolerance discriminates sharply.
    let (target, _target_surface_indices) =
        fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_TARGET, 4);
    assert!(
        !surface_indices.is_empty(),
        "bracket(n=4) must yield surface nodes; without them the morph has no \
         Dirichlet data and (a) above would be the only constraint left"
    );
    let mut worst = (0u32, 0.0_f64);
    for &i in &surface_indices {
        let want = target
            .vertex_f64(i)
            .expect("surface index must be in range for the target fixture");
        let got = morphed
            .vertex_f64(i)
            .expect("surface index must be in range for the morphed mesh");
        let delta = (0..3)
            .map(|axis| (want[axis] - got[axis]).abs())
            .fold(0.0_f64, f64::max);
        if delta > worst.1 {
            worst = (i, delta);
        }
    }
    assert!(
        worst.1 <= PRESCRIBED_TOLERANCE,
        "prescribed surface node {} missed its target by {:e} (> {:e}): the \
         morph moved the mesh but did not honour the Dirichlet data, so the \
         harness would be timing a solve of a different problem",
        worst.0,
        worst.1,
        PRESCRIBED_TOLERANCE
    );
}

/// How far a Dirichlet-prescribed node may sit from its prescribed position.
///
/// `elasticity_morph` pins those DOFs by row elimination and narrows
/// `old + u` back to `f32`. Because `u` is set to exactly
/// `target_f64 - source_f64` over two `f32`-representable values, a satisfied
/// node reproduces the target's own `f32` coordinate essentially bit-for-bit:
/// the measured worst case at n=4 is 5.5e-26, i.e. the f32-rounding floor
/// (~1.2e-7 at coordinate magnitude 1) is never actually reached.
///
/// The bound is nonetheless set well ABOVE that measurement rather than at
/// it. Pinning a tolerance to a single observed residual would make this test
/// a detector of harmless CG-residual drift across hosts and profiles; 1e-6
/// still sits four orders of magnitude below the 0.01 fillet step being
/// prescribed, so no morph that ignores the Dirichlet data can slip under it.
const PRESCRIBED_TOLERANCE: f64 = 1e-6;

// ── the gmsh from-scratch arm ────────────────────────────────────────────────

/// One timed from-scratch tetrahedralisation, reported as data rather than as
/// a pass/fail — the gmsh-arm counterpart to [`MorphMeasurement`].
///
/// `mesh_size` records what was ASKED; `tets`/`nodes` record what was
/// ACHIEVED. gmsh maps `mesh_size` onto `Mesh.MeshSizeMin`/`MeshSizeMax` as a
/// target, not a guarantee, so the two are reported side by side and the
/// driver prints both: a reader never has to trust an a-priori size-to-count
/// estimate, because the achieved count is right there.
///
/// `result` is carried unconverted for the same reason [`MorphMeasurement`]
/// does: a failed rung is still a data point about where the from-scratch arm
/// stops working, and `unwrap`ping it here would destroy the rest of the
/// ladder along with it. On the `Err` path `tets` and `nodes` are zero rather
/// than absent, so a failed rung still prints in the same columns as a
/// successful one.
#[cfg(has_gmsh)]
#[derive(Debug)]
struct GmshMeasurement {
    /// The `mesh_size` this rung requested.
    mesh_size: f64,
    /// P1 tet count actually produced (0 when `result` is `Err`).
    tets: usize,
    /// Node count actually produced (0 when `result` is `Err`).
    nodes: usize,
    /// Wall-clock of the `mesh_to_volume` call ALONE — see
    /// [`gmsh_tetrahedralise`] for what is deliberately excluded.
    elapsed: Duration,
    /// gmsh's own result, uninterpreted.
    result: Result<VolumeMesh, GeometryError>,
}

/// Tetrahedralise `surface` from scratch at a requested characteristic edge
/// length, timing only the mesher.
///
/// ## What the timed region covers
///
/// Only `mesh_to_volume`. Boundary-surface extraction stays outside it, for
/// the same reason the morph arm excludes fixture construction: it is input
/// preparation shared by neither arm's real workload, and folding it in would
/// bias the ratio this harness exists to report.
///
/// ## Why the inherent method rather than the trait
///
/// The `mesh_size` knob lives ONLY on `GmshKernel::mesh_to_volume`. The
/// `&dyn GeometryKernel` trait path (`mesh_surface_to_volume`) hardcodes
/// `MeshingOptions::default()`, i.e. auto-size derived from the smallest
/// input triangle edge — on this bracket that is roughly 650K tets with no
/// way to dial it, which cannot produce a 10K rung at all. And
/// `refine_volume_with_size_field` is a different algorithm (size-field
/// refinement of an existing mesh), not the from-scratch tetrahedralisation
/// this harness is timing.
///
/// ## `deterministic: true`
///
/// Forces single-threaded HXT. That is what makes rung-to-rung counts
/// reproducible (`crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:402-419`
/// records that the multi-threaded variant needed a ±10% count budget where
/// the single-threaded one needs ±1%), and it is the apples-to-apples
/// counterpart to the morph arm, which `src/elasticity.rs` hardcodes serial.
/// Neither arm characterises the parallel path.
///
/// ## Three things this deliberately does not do
///
/// - It does NOT acquire `reify_kernel_gmsh::init::GMSH_LOCK`.
///   `mesh_to_volume` takes that lock internally, so holding it at the call
///   site self-deadlocks — warned about explicitly at
///   `crates/reify-kernel-gmsh/tests/volume_fill_fraction.rs:143-145` and
///   `mesh_plane_2d_tests.rs:22-23`.
/// - It does NOT call `ffi::finalize()`.
/// - It never deliberately feeds gmsh an open or unmeshable surface.
///   `mesh_to_volume_tests.rs:540-546` records that a failed HXT
///   `mesh_generate` leaves thread-local HXT state that SURVIVES
///   `gmshClear()` and corrupts the *next* call's output — which comes back
///   as 0 tets rather than as an error. In a binary that sweeps a whole
///   ladder through one process, one poisoned rung would silently zero every
///   rung after it, so every surface handed to this function comes from
///   [`boundary::boundary_surface`], whose closedness is pinned by an
///   always-on test.
#[cfg(has_gmsh)]
fn gmsh_tetrahedralise(surface: &reify_ir::Mesh, mesh_size: f64) -> GmshMeasurement {
    use reify_ir::ElementOrderTag;
    use reify_kernel_gmsh::{GmshKernel, MeshingOptions};

    let kernel = GmshKernel::new();
    let options = MeshingOptions {
        mesh_size: Some(mesh_size),
        deterministic: true,
        ..Default::default()
    };

    let started = Instant::now();
    let result = kernel.mesh_to_volume(surface, &options, ElementOrderTag::P1);
    let elapsed = started.elapsed();

    let (tets, nodes) = match &result {
        Ok(volume) => (
            volume.tet_indices().map_or(0, |t| t.len() / 4),
            volume.vertices.len() / 3,
        ),
        Err(_) => (0, 0),
    };

    GmshMeasurement {
        mesh_size,
        tets,
        nodes,
        elapsed,
        result,
    }
}

/// One timed from-scratch tetrahedralisation must actually produce a P1 tet
/// mesh at the resolution it was asked for, and must report counts that match
/// the mesh it produced.
///
/// Deliberately coarse (`mesh_size = 0.06` on the n=4 bracket, whose whole
/// thickness is `THICKNESS = 0.2`) so the always-on suite pays for a handful
/// of tets rather than for a calibration rung. The 10K and 100K rungs belong
/// to the `#[ignore]`d driver.
///
/// The surface handed to gmsh is [`boundary::boundary_surface`]'s output,
/// which the sibling test above pins as closed and consistently wound — that
/// ordering matters, because an open surface would not merely fail here, it
/// would poison the rest of the binary (see [`gmsh_tetrahedralise`]).
#[cfg(has_gmsh)]
#[test]
fn gmsh_tetrahedralise_produces_tets_at_a_requested_mesh_size() {
    use reify_ir::ElementOrderTag;

    let (mesh, _surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, 4);
    let surface = boundary::boundary_surface(&mesh);

    let measurement = gmsh_tetrahedralise(&surface, 0.06);

    let volume = match &measurement.result {
        Ok(volume) => volume,
        Err(error) => panic!(
            "gmsh_tetrahedralise must succeed on the extracted bracket boundary at a \
             coarse size: the surface is closed and consistently wound (pinned by \
             `bracket_boundary_surface_is_closed_outward_wound_and_fully_referenced`), \
             so a failure here is a rig or linkage fault rather than a geometry one — \
             got {error:?}"
        ),
    };

    assert_eq!(
        volume.element_order(),
        Some(ElementOrderTag::P1),
        "the gmsh arm must produce P1 tets — the morph arm it is compared against is \
         P1-only, and a P2 mesh would put the two arms on different element counts"
    );

    let tets = volume
        .tet_indices()
        .expect("gmsh must return tet connectivity for a volume mesh");
    assert!(
        !tets.is_empty(),
        "gmsh returned an empty tet table; there is no wall-clock to attribute"
    );

    // The reported counts are what the driver prints and what
    // `nearest_by_tet_count` joins the two ladders on, so a count that does
    // not describe the mesh it came from would silently corrupt every ratio
    // derived downstream.
    assert_eq!(
        measurement.tets,
        tets.len() / 4,
        "reported tet count must match the returned mesh"
    );
    assert_eq!(
        measurement.nodes,
        volume.vertices.len() / 3,
        "reported node count must match the returned mesh"
    );

    assert!(
        measurement.elapsed > Duration::ZERO,
        "the clock was never read — `elapsed` is {:?}",
        measurement.elapsed
    );

    // The requested size is carried through verbatim so a rung is labelled by
    // what was ASKED, while `tets`/`nodes` record what was ACHIEVED. The
    // driver prints both precisely because gmsh does not promise they agree.
    assert_eq!(
        measurement.mesh_size, 0.06,
        "the measurement must report the mesh size it was asked for"
    );
}

/// A stub build must still run a test here rather than silently compiling the
/// whole gmsh arm to nothing.
///
/// This is the failure mode `crates/reify-solver-elastic/tests/aposteriori_validation.rs:42-70`
/// warns about: a `#[cfg(has_gmsh)]`-gated test in a crate that never derived
/// `has_gmsh` for itself vanishes, and a vanished test reports as a pass. The
/// assertion is deliberately on `reify_kernel_gmsh::GMSH_AVAILABLE` — the
/// gmsh crate's OWN view of its build — so this test disagrees loudly with
/// its `#[cfg(has_gmsh)]` sibling if this crate's `build.rs` detection ever
/// drifts from the gmsh crate's.
#[cfg(not(has_gmsh))]
#[test]
// `GMSH_AVAILABLE` is `pub const GMSH_AVAILABLE: bool = cfg!(has_gmsh)` (in
// `crates/reify-kernel-gmsh/src/lib.rs`), so this assertion is constant-valued
// BY CONSTRUCTION and clippy's `assertions_on_constants` fires on it — turning `scripts/verify.sh`'s `cargo clippy --all-targets --
// -D warnings` pass into a hard failure on precisely the stub-build host
// this test exists to serve. The constant-ness IS the point: the value under
// test is another crate's view of its own build, which is not knowable when
// this line is written.
//
// Deliberately NOT hoisted into a `const { … }` block (clippy's own
// suggestion): that would make a detected drift a COMPILE error, taking the
// whole test binary down and leaving a stub-build host with no test results
// at all — where today it gets one loudly failing test and every sibling
// still reported.
#[allow(clippy::assertions_on_constants)]
fn gmsh_arm_is_absent_in_a_stub_build() {
    assert!(
        !reify_kernel_gmsh::GMSH_AVAILABLE,
        "this crate's build.rs did NOT set `has_gmsh`, yet the gmsh crate reports \
         GMSH_AVAILABLE — the two detections have drifted, and the entire gmsh arm \
         of this harness is silently compiled out while gmsh is in fact usable"
    );
}

/// The `mesh_size` rungs the gmsh arm sweeps.
///
/// ONE shared ladder, not one per scale: both scales are read off the same
/// sweep, so the two paired ratios come from a single monotone sequence and
/// cannot be accused of having been tuned per band.
///
/// Rung placement (an estimate, and only an estimate). The bracket at
/// `arm_length = 1.0`, `thickness = 0.2`, `fillet_radius = 0.05` has volume
/// `~= 2*(1.0*0.2*0.2) - 0.2^3 - fillet-cut ~= 0.0719`. A uniform tet mesh at
/// characteristic length `h` holds `~= 8.49 * V / h^3` tets (a regular tet of
/// edge `h` has volume `h^3 / (6*sqrt(2))`), so 10K tets is `h ~= 0.039` and
/// 100K is `h ~= 0.018`. The ladder brackets both with margin either side.
///
/// Nothing downstream depends on that estimate being right. Each rung's
/// ACHIEVED count is measured and printed, and [`nearest_by_tet_count`] pairs
/// on achieved counts — so if the estimate is off, the printed table shows it
/// and the pairing still selects correctly. The estimate places the rungs; it
/// never interprets them.
#[cfg(has_gmsh)]
const MESH_SIZE_LADDER: [f64; 7] = [0.060, 0.045, 0.035, 0.028, 0.022, 0.017, 0.014];

/// Select the successful gmsh rung whose achieved tet count is nearest
/// `target`.
///
/// This is the SOLE place the two independently-swept ladders are joined. The
/// morph arm is swept by `n` and the gmsh arm by `mesh_size`; neither knows
/// the other's achieved counts, so the two never land on the same number and
/// a comparison has to nominate a pairing rule. Making that rule one small
/// pure function — rather than an inline scan inside the driver — is what
/// lets it be tested at all, and the driver prints the residual count
/// mismatch alongside every ratio so a reader can see how good the pairing
/// actually was.
///
/// Failed rungs are filtered out before selection: a failed rung reports
/// `tets == 0` and a wall-clock that measures how long gmsh took to give up,
/// so pairing against one would fabricate a ratio out of a zero.
///
/// An exact tie — a target equidistant between two succeeding rungs — resolves
/// to the EARLIER rung in `entries`, which is `min_by_key`'s first-wins rule
/// applied to a [`MESH_SIZE_LADDER`] ordered coarse -> fine. That direction is
/// the conservative one, and deliberately so: the earlier rung is the coarser
/// one, so it is the CHEAPER gmsh arm, and the driver prints
/// `gmsh_wall / morph_wall`. Resolving a tie toward the coarser rung therefore
/// understates that ratio — it can only make the morph look worse against
/// #2953's `>=10x` premise, never better. A pairing rule that is going to be
/// arbitrary at a tie should be arbitrary in the direction that cannot flatter
/// the result it is used to judge.
///
/// Returns `Option` rather than panicking so an empty or wholly-failed ladder
/// degrades to "no paired ratio available" and the driver still prints both
/// raw ladders — the run's other measurements survive the loss of the join.
#[cfg(has_gmsh)]
fn nearest_by_tet_count(entries: &[GmshMeasurement], target: usize) -> Option<&GmshMeasurement> {
    entries
        .iter()
        .filter(|entry| entry.result.is_ok())
        .min_by_key(|entry| entry.tets.abs_diff(target))
}

/// The joiner must select the rung minimising `|tets - target|`, must skip
/// failed rungs, and must degrade to `None` rather than panicking.
///
/// Driven entirely by synthetic values: no gmsh call and no meshing, because
/// what is under test is the selection rule, not any particular count. Held
/// non-ignored for that reason — it is pure arithmetic and costs nothing.
///
/// The load-bearing case is the third one. The two ladders are swept
/// INDEPENDENTLY — the morph arm by `n`, the gmsh arm by `mesh_size` — so
/// their counts never coincide, and a naive "largest rung at or below the
/// target" scan silently reports a ratio taken at a materially coarser
/// gmsh rung than the morph it is divided by. That inflates the morph arm's
/// apparent advantage in exactly the direction #2953's threshold is
/// sensitive to, which is why it is pinned here rather than left to review.
#[cfg(has_gmsh)]
#[test]
fn nearest_by_tet_count_selects_the_closest_rung() {
    fn rung(mesh_size: f64, tets: usize, ok: bool) -> GmshMeasurement {
        GmshMeasurement {
            mesh_size,
            tets,
            nodes: tets / 5,
            elapsed: Duration::from_millis(1),
            result: if ok {
                Ok(VolumeMesh {
                    vertices: Vec::new(),
                    connectivity: reify_ir::VolumeConnectivity::Tet {
                        indices: Vec::new(),
                        order: reify_ir::ElementOrderTag::P1,
                    },
                    normals: None,
                    boundary: None,
                })
            } else {
                Err(reify_ir::GeometryError::OperationFailed(
                    "synthetic failed rung".to_string(),
                ))
            },
        }
    }

    // Empty ladder — a stub build or a wholly failed sweep must degrade to
    // "no paired ratio available", not abort the driver mid-run.
    assert!(
        nearest_by_tet_count(&[], 10_000).is_none(),
        "an empty ladder must yield None"
    );

    let ladder = [
        rung(0.060, 4_000, true),
        rung(0.035, 9_000, true),
        rung(0.028, 12_000, true),
        rung(0.014, 95_000, true),
    ];

    // Exact hit.
    let hit = nearest_by_tet_count(&ladder, 12_000).expect("a non-empty ok ladder must select");
    assert_eq!(hit.tets, 12_000, "an exact match must select itself");

    // Nearest is BELOW the target.
    let below = nearest_by_tet_count(&ladder, 9_400).expect("must select");
    assert_eq!(
        below.tets, 9_000,
        "9,400 is 400 from 9,000 and 2,600 from 12,000"
    );

    // Nearest is ABOVE the target — the case a "largest rung below target"
    // scan gets wrong, and the reason this test exists.
    let above = nearest_by_tet_count(&ladder, 11_000).expect("must select");
    assert_eq!(
        above.tets, 12_000,
        "11,000 is 1,000 from 12,000 but 2,000 from 9,000 — a below-only scan \
         would wrongly pair against the 9,000 rung and overstate the morph arm's \
         advantage by pairing it against a coarser remesh than it was matched to"
    );

    // Failed rungs are excluded even when they are numerically nearest: a
    // failed rung reports `tets == 0` and carries no meaningful wall-clock, so
    // pairing against one would fabricate a ratio out of a zero.
    let with_failure = [
        rung(0.028, 12_000, false),
        rung(0.035, 9_000, true),
        rung(0.060, 4_000, true),
    ];
    let selected = nearest_by_tet_count(&with_failure, 12_000).expect("must select an ok rung");
    assert_eq!(
        selected.tets, 9_000,
        "the numerically exact rung failed, so the nearest SUCCEEDING rung must be \
         chosen instead of fabricating a ratio against a failed one"
    );

    // An exact tie must resolve to the COARSER rung — the earlier one in a
    // coarse -> fine ladder. Unpinned, this is the one input on which the
    // selection rule is arbitrary, and the arbitrary choice has a direction:
    // the coarser rung is the cheaper gmsh arm, so it understates the printed
    // `gmsh/morph` ratio rather than inflating it.
    let tied = [rung(0.035, 9_000, true), rung(0.028, 11_000, true)];
    let tie = nearest_by_tet_count(&tied, 10_000).expect("must select");
    assert_eq!(
        tie.tets, 9_000,
        "10,000 is 1,000 from both rungs; the tie must break toward the coarser \
         one, which is the direction that cannot flatter the morph arm"
    );

    // A ladder with no successful rung at all is indistinguishable from an
    // empty one, and must degrade the same way.
    let all_failed = [rung(0.028, 12_000, false), rung(0.035, 9_000, false)];
    assert!(
        nearest_by_tet_count(&all_failed, 12_000).is_none(),
        "a ladder whose every rung failed must yield None, not a failed rung"
    );
}

// ── the driver ───────────────────────────────────────────────────────────────

/// Measure both arms at both scales on one host and print everything.
///
/// ## Why this test asserts nothing
///
/// It is a measurement, not a bound. A single unrepeated run on one host
/// carries no statistics, so any threshold asserted from it would be pinning
/// noise. Its output is the deliverable; its exit status means only "the rig
/// ran".
///
/// ## Why there is no RED commit for this
///
/// An `#[ignore]`d test never runs under `cargo test`, so it can be neither
/// red nor green, and it asserts nothing by design — there is no proposition
/// to fail. That is not a gap in the TDD chain: every helper composed below
/// was driven RED -> GREEN on its own, and this function only sequences them
/// and formats output. Nothing here is unwrapped, so a `SolverNotConverged`
/// or a gmsh `GeometryError` is printed as data rather than aborting the run
/// that produced it.
///
/// ## How to run
///
/// ```text
/// cargo test -p reify-mesh-morph --test morph_scale_characterisation -- --ignored --nocapture --test-threads=1
/// ```
#[test]
#[ignore = "characterisation harness; run explicitly with --ignored"]
fn gmsh_from_scratch_vs_morph_wall_clock_at_10k_and_100k() {
    eprintln!("[task-6638] ── characterisation harness ──────────────────────────");
    eprintln!(
        "[task-6638] fixture: bracket(arm_length={ARM_LENGTH}, thickness={THICKNESS}, \
         fillet_radius={FILLET_BASE})"
    );

    // ── 1. the surfaces both scales are read off ─────────────────────────────
    let mut surfaces = Vec::new();
    for n in [N_10K, N_100K] {
        let (mesh, _surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, n);
        let surface = boundary::boundary_surface(&mesh);
        eprintln!(
            "[task-6638] surface n={n:<3} volume_tets={:<7} surface_tris={:<7} surface_verts={}",
            mesh.tet_indices().map_or(0, |t| t.len() / 4),
            surface.indices.len() / 3,
            surface.vertices.len() / 3,
        );
        surfaces.push(surface);
    }

    // ── 2. the gmsh ladder ───────────────────────────────────────────────────
    #[cfg(has_gmsh)]
    let ladder: Vec<GmshMeasurement> = {
        // Swept against the N_100K-derived surface. One surface for the whole
        // ladder, and the finer of the two: a coarse boundary would cap the
        // achievable interior resolution at the fine rungs, so the fine
        // surface is the better input at BOTH target sizes and using one
        // keeps the rungs comparable to each other.
        let surface = &surfaces[1];
        eprintln!(
            "[task-6638] gmsh ladder input: the n={N_100K} surface ({} tris) for every rung",
            surface.indices.len() / 3
        );
        MESH_SIZE_LADDER
            .iter()
            .map(|&mesh_size| {
                let measurement = gmsh_tetrahedralise(surface, mesh_size);
                eprintln!(
                    "[task-6638] gmsh  mesh_size={:.3} tets={:<8} nodes={:<8} wall={:>12?} {}",
                    measurement.mesh_size,
                    measurement.tets,
                    measurement.nodes,
                    measurement.elapsed,
                    match &measurement.result {
                        Ok(_) => "Ok".to_string(),
                        Err(error) => format!("Err({error})"),
                    }
                );
                measurement
            })
            .collect()
    };

    #[cfg(not(has_gmsh))]
    eprintln!(
        "[task-6638] gmsh arm skipped: stub build (no libgmsh). The morph arm below \
         is a HALF table — there is no from-scratch baseline to pair it against, and \
         no ratio may be derived from this run."
    );

    // ── 3. the morph arm ─────────────────────────────────────────────────────
    let morphs: Vec<MorphMeasurement> = [N_10K, N_100K]
        .into_iter()
        .map(|n| {
            let measurement = morph_once(n);
            eprintln!(
                "[task-6638] morph n={:<3} tets={:<8} nodes={:<8} dof={:<8} wall={:>12?} {}",
                measurement.n,
                measurement.tets,
                measurement.nodes,
                measurement.nodes * 3,
                measurement.elapsed,
                match &measurement.result {
                    Ok(_) => "Ok".to_string(),
                    Err(failure) => format!("Err({failure:?})"),
                }
            );
            measurement
        })
        .collect();

    // Step 4 below is `morphs`'s ONLY reader and is `#[cfg(has_gmsh)]`, so in
    // a stub build the binding has no reader at all — yet it must still exist,
    // because constructing it is what runs and prints the morph arm (the half
    // of the table a stub build can still produce). Without this marker rustc
    // reports `unused variable: morphs` there, and `scripts/verify.sh`'s
    // `cargo clippy --all-targets -- -D warnings` pass turns that warning into
    // a failure — on exactly the host configuration
    // `gmsh_arm_is_absent_in_a_stub_build` exists to keep honest.
    #[cfg(not(has_gmsh))]
    let _ = &morphs;

    // ── 4. the count-matched pairing — the number #2953 actually needs ───────
    #[cfg(has_gmsh)]
    for morph in &morphs {
        match nearest_by_tet_count(&ladder, morph.tets) {
            Some(gmsh) => {
                let mismatch_pct =
                    100.0 * (gmsh.tets as f64 - morph.tets as f64) / morph.tets as f64;
                let ratio = gmsh.elapsed.as_secs_f64() / morph.elapsed.as_secs_f64();
                eprintln!(
                    "[task-6638] PAIR  n={:<3} morph_tets={:<8} gmsh_tets={:<8} \
                     mismatch={:+.1}% morph_wall={:>12?} gmsh_wall={:>12?} \
                     gmsh/morph={:.2}x (gmsh mesh_size={:.3}){}",
                    morph.n,
                    morph.tets,
                    gmsh.tets,
                    mismatch_pct,
                    morph.elapsed,
                    gmsh.elapsed,
                    ratio,
                    gmsh.mesh_size,
                    if morph.result.is_err() {
                        "  [morph FAILED — this ratio divides by a time-to-give-up, \
                         not by a time-to-solve]"
                    } else {
                        ""
                    },
                );
            }
            None => eprintln!(
                "[task-6638] PAIR  n={:<3} morph_tets={:<8} no paired ratio available \
                 (no succeeding gmsh rung to match against)",
                morph.n, morph.tets,
            ),
        }
    }

    // ── 5. what a consumer of these numbers must carry with them ─────────────
    eprintln!("[task-6638] ── caveats ────────────────────────────────────────────");
    eprintln!("[task-6638] * ONE host, ONE run per point. No repetition, no variance,");
    eprintln!("[task-6638]   no warm-up discard — do not read a small ratio difference");
    eprintln!("[task-6638]   as signal.");
    eprintln!("[task-6638] * The morph arm is forced SERIAL: src/elasticity.rs hardcodes");
    eprintln!("[task-6638]   AssemblyMode::Deterministic and SolverMode::Deterministic, and");
    eprintln!("[task-6638]   elasticity_morph exposes no assembly/solve split, so the time");
    eprintln!("[task-6638]   above is the combined call and cannot be attributed between");
    eprintln!("[task-6638]   assembly and CG.");
    eprintln!("[task-6638] * The gmsh arm is forced SINGLE-THREADED (deterministic: true) as");
    eprintln!("[task-6638]   the apples-to-apples counterpart. NEITHER arm characterises the");
    eprintln!("[task-6638]   parallel path that both PRD figures also quote.");
    eprintln!("[task-6638] * Each pairing carries a residual count mismatch, printed above.");
    eprintln!("[task-6638]   The ladders are swept independently and never land on the same");
    eprintln!("[task-6638]   count, so a ratio is only as meaningful as its mismatch is small.");
    eprintln!("[task-6638] * A morph Err is still timed. Time-to-max-iter is data, but a ratio");
    eprintln!("[task-6638]   against it measures how long the solver took to GIVE UP, not how");
    eprintln!("[task-6638]   long it took to solve.");
    eprintln!("[task-6638] ───────────────────────────────────────────────────────");
}
