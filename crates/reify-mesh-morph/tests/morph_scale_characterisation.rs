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

/// The tet -> boundary-surface extractor that feeds the gmsh arm must emit
/// exactly the once-occurring tet faces, wound outward, compacted onto only
/// the vertices it actually references.
///
/// The assertion target is `reify_ir::Mesh::validate`
/// (`crates/reify-ir/src/geometry.rs:3187`) rather than a hand-rolled Euler
/// check: it is precisely the producer-obligation set gmsh's preflight
/// demands — finite, index-valid, non-degenerate, closed, and consistently
/// wound on the position-welded quotient.
///
/// Cheap and always-on: n=4, the existing `calibration.rs` cost point.
#[test]
fn bracket_boundary_surface_is_closed_outward_wound_and_fully_referenced() {
    use std::collections::{HashMap, HashSet};

    let (mesh, _surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, 4);
    let surface = fixtures::boundary_surface(&mesh);

    // (1) The full mesh contract. Naming the Err variant means a contract
    // violation reports which obligation failed rather than "assert failed".
    if let Err(violation) = surface.validate(1e-6) {
        panic!(
            "boundary_surface(bracket(n=4)) violates the mesh contract, so gmsh's \
             preflight would reject it: {violation:?}"
        );
    }

    assert_eq!(
        surface.indices.len() % 3,
        0,
        "boundary_surface must emit whole triangles; got {} indices",
        surface.indices.len()
    );

    // (2) Triangle count == number of tet faces occurring exactly once,
    // recomputed here from `tet_indices` so the test does not merely restate
    // the implementation.
    let tets = mesh
        .tet_indices()
        .expect("bracket must expose tet connectivity");
    let mut face_counts: HashMap<[u32; 3], usize> = HashMap::new();
    for tet in tets.chunks_exact(4) {
        for &[i, j, k] in &[[0usize, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
            let mut key = [tet[i], tet[j], tet[k]];
            key.sort_unstable();
            *face_counts.entry(key).or_insert(0) += 1;
        }
    }
    let expected_tris = face_counts.values().filter(|&&c| c == 1).count();
    assert_eq!(
        surface.indices.len() / 3,
        expected_tris,
        "boundary_surface must emit exactly the once-occurring tet faces"
    );

    // (3) Compaction: every emitted vertex is referenced by some triangle,
    // i.e. the extractor remaps rather than carrying interior nodes through.
    let used: HashSet<u32> = surface.indices.iter().copied().collect();
    assert_eq!(
        used.len(),
        surface.vertices.len() / 3,
        "boundary_surface must carry only referenced vertices; {} emitted but only \
         {} referenced (interior nodes would be handed to gmsh for nothing)",
        surface.vertices.len() / 3,
        used.len()
    );

    // (4) Compaction is a strict narrowing of the volume mesh's vertex table.
    assert!(
        surface.vertices.len() / 3 <= mesh.vertices.len() / 3,
        "boundary_surface emitted {} vertices from a {}-vertex volume mesh",
        surface.vertices.len() / 3,
        mesh.vertices.len() / 3
    );
}

// ── Morph arm ────────────────────────────────────────────────────────────────

/// One timed `elasticity_morph` call, reported as data rather than as a
/// pass/fail.
///
/// `result` is carried unconverted on purpose. At the 100K scale the serial
/// Jacobi-CG (`max_iter` 1000, tol 1e-8, ~54K DOF) may legitimately return
/// [`ElasticityFailure::SolverNotConverged`], and the time taken to reach
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
    /// Node count of the source mesh.
    nodes: usize,
    /// Displacement degrees of freedom: `3 * nodes`.
    dof: usize,
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
        dof: 3 * nodes,
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
    let (source, _surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, 4);
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
    assert_eq!(
        measurement.dof,
        3 * measurement.nodes,
        "the elasticity morph carries three displacement DOF per node"
    );

    assert!(
        measurement.elapsed > Duration::ZERO,
        "the clock was never read — `elapsed` is {:?}",
        measurement.elapsed
    );
}

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
///   [`fixtures::boundary_surface`], whose closedness is pinned by an
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
/// The surface handed to gmsh is [`fixtures::boundary_surface`]'s output,
/// which the sibling test above pins as closed and consistently wound — that
/// ordering matters, because an open surface would not merely fail here, it
/// would poison the rest of the binary (see [`gmsh_tetrahedralise`]).
#[cfg(has_gmsh)]
#[test]
fn gmsh_tetrahedralise_produces_tets_at_a_requested_mesh_size() {
    use reify_ir::ElementOrderTag;

    let (mesh, _surface_indices) = fixtures::bracket(ARM_LENGTH, THICKNESS, FILLET_BASE, 4);
    let surface = fixtures::boundary_surface(&mesh);

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
fn gmsh_arm_is_absent_in_a_stub_build() {
    assert!(
        !reify_kernel_gmsh::GMSH_AVAILABLE,
        "this crate's build.rs did NOT set `has_gmsh`, yet the gmsh crate reports \
         GMSH_AVAILABLE — the two detections have drifted, and the entire gmsh arm \
         of this harness is silently compiled out while gmsh is in fact usable"
    );
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

    // A ladder with no successful rung at all is indistinguishable from an
    // empty one, and must degrade the same way.
    let all_failed = [rung(0.028, 12_000, false), rung(0.035, 9_000, false)];
    assert!(
        nearest_by_tet_count(&all_failed, 12_000).is_none(),
        "a ladder whose every rung failed must yield None, not a failed rung"
    );
}
