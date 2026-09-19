//! Integration tests for [`reify_kernel_gmsh::refine_volume_with_size_field`].
//!
//! Only compiled and run when `cfg(has_gmsh)` is set by `build.rs` (i.e. when
//! libgmsh was found at build time). On stub builds this file is empty and the
//! test binary contains zero tests, preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

// The clamp probe and its serialising mutex are shared verbatim with
// `tests/mesh_to_volume_clamp_hermeticity.rs`, the other half of this
// discipline. Declared by path rather than through `common/mod.rs`, which
// #6387 reduced to a re-export shim over `reify_test_support::fixtures` and
// which is scheduled for deletion; see `common/clamp_probe.rs` for why one
// copy matters.
#[path = "common/clamp_probe.rs"]
mod clamp_probe;

use clamp_probe::{
    CLAMP_TEST_ORDER, GMSH_CLAMP_DEFAULTS, poison_global_mesh_size_clamp, probe_triangle_count,
    set_global_mesh_size_clamp,
};
use reify_ir::{ElementOrderTag, Mesh, VolumeConnectivity, VolumeMesh};
use reify_kernel_gmsh::{BackgroundSizeField, MeshingOptions, refine_volume_with_size_field};
use reify_test_support::fixtures::unit_cube_mesh;

/// A `unit_cube_mesh` scaled uniformly about the origin, i.e. the box
/// `[0,scale]^3`.
///
/// Used by the non-uniform test, which needs a domain with genuine interior
/// room to coarsen — see its docstring.
fn scaled_cube_mesh(scale: f32) -> Mesh {
    let mut cube = unit_cube_mesh();
    for v in &mut cube.vertices {
        *v *= scale;
    }
    cube
}

/// Kuhn decomposition of `[0, scale]^3` over an `n^3` lattice of cells —
/// `(n+1)^3` vertices, `6n^3` tets, each cell cut into the six tets that share
/// its main diagonal.
///
/// A sizing mesh only: it carries the size field into the kernel and is never
/// compared against the result.
fn kuhn_lattice_box_vm(scale: f64, n: usize) -> VolumeMesh {
    let side = n + 1;
    let vid = |i: usize, j: usize, k: usize| ((k * side + j) * side + i) as u32;
    let coord = |i: usize| (scale * i as f64 / n as f64) as f32;

    let mut vertices = Vec::with_capacity(3 * side * side * side);
    for k in 0..side {
        for j in 0..side {
            for i in 0..side {
                vertices.extend_from_slice(&[coord(i), coord(j), coord(k)]);
            }
        }
    }

    const AXIS_ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut indices = Vec::with_capacity(4 * 6 * n * n * n);
    for ck in 0..n {
        for cj in 0..n {
            for ci in 0..n {
                for order in AXIS_ORDERS {
                    let mut step = [0_usize; 3];
                    indices.push(vid(ci, cj, ck));
                    for axis in order {
                        step[axis] = 1;
                        indices.push(vid(ci + step[0], cj + step[1], ck + step[2]));
                    }
                }
            }
        }
    }

    VolumeMesh {
        vertices,
        connectivity: VolumeConnectivity::Tet {
            indices,
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// A [`BackgroundSizeField`] over `[0, scale]^3`, sized by position.
///
/// These tests were written against a per-SURFACE-vertex hint slice, which the
/// kernel no longer takes (task #7447 — a boundary field discards every
/// interior value). Each one's positional rule is preserved verbatim and
/// evaluated at the vertices of a sizing lattice spanning the same box, so what
/// moves is the plumbing, not which hint applies where.
///
/// The lattice puts a vertex plane at every quarter of the box, which is what
/// the non-uniform fixture's split at `SCALE / 2` needs; a uniform field is
/// unaffected by the resolution.
fn box_size_field(scale: f64, size_at: impl Fn(f64, f64, f64) -> f64) -> BackgroundSizeField {
    const LATTICE_CELLS: usize = 4;
    let vm = kuhn_lattice_box_vm(scale, LATTICE_CELLS);
    let sizes: Vec<f64> = vm
        .vertices
        .chunks_exact(3)
        .map(|xyz| size_at(xyz[0] as f64, xyz[1] as f64, xyz[2] as f64))
        .collect();
    BackgroundSizeField::from_tet_mesh(&vm, &sizes)
        .unwrap_or_else(|e| panic!("sizing lattice must yield a valid size field: {e:?}"))
}

/// Remesh `cube` under `size_field` and return the P1 tet count.
fn refine_tet_count(cube: &Mesh, size_field: &BackgroundSizeField, opts: &MeshingOptions) -> usize {
    let vm = refine_volume_with_size_field(cube, size_field, opts, ElementOrderTag::P1)
        .unwrap_or_else(|e| panic!("refine_volume_with_size_field must succeed: {e:?}"));
    vm.tet_indices().expect("P1 tet mesh").len() / 4
}

/// Per-side element-size statistics for a P1 tet mesh split by centroid `x`.
///
/// Index 0 is the "marked" side (`centroid_x < split_x`), index 1 the
/// unmarked side. Each tet's size proxy is its own mean edge length — the
/// quantity directly comparable to a requested characteristic-length hint.
struct SplitStats {
    counts: [usize; 2],
    /// Mean over the side's tets of each tet's mean edge length.
    mean_edge: [f64; 2],
    /// Largest single tet mean edge length on the side. This — not the mean —
    /// is what `Mesh.MeshSizeMax` bounds, so it is the cap-sensitive statistic.
    max_edge: [f64; 2],
}

/// Partition a P1 tet mesh by centroid `x` against `split_x`.
fn split_by_centroid_x(vm: &reify_ir::VolumeMesh, split_x: f64) -> SplitStats {
    // The six edges of a tet, as index pairs into its four corners.
    const TET_EDGES: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

    let verts = &vm.vertices;
    let tets = vm.tet_indices().expect("P1 tet mesh");

    let mut counts = [0_usize; 2]; // [marked (x<split_x), unmarked]
    let mut edge_len_sums = [0.0_f64; 2];
    let mut max_edge = [0.0_f64; 2];
    for tet in tets.chunks_exact(4) {
        let p: [[f64; 3]; 4] = std::array::from_fn(|k| {
            let b = 3 * tet[k] as usize;
            [verts[b] as f64, verts[b + 1] as f64, verts[b + 2] as f64]
        });
        let centroid_x = p.iter().map(|q| q[0]).sum::<f64>() / 4.0;
        let side = usize::from(centroid_x >= split_x);

        let mut edge_sum = 0.0;
        for (a, b) in TET_EDGES {
            let d = [p[a][0] - p[b][0], p[a][1] - p[b][1], p[a][2] - p[b][2]];
            edge_sum += (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        }
        let tet_size = edge_sum / TET_EDGES.len() as f64;
        counts[side] += 1;
        edge_len_sums[side] += tet_size;
        max_edge[side] = max_edge[side].max(tet_size);
    }

    let mean_edge = [
        edge_len_sums[0] / counts[0].max(1) as f64,
        edge_len_sums[1] / counts[1].max(1) as f64,
    ];
    SplitStats {
        counts,
        mean_edge,
        max_edge,
    }
}

/// A finer uniform size field produces strictly more tets **even when the
/// process-global mesh-size clamp has been poisoned by an earlier call**.
///
/// gmsh's option table is process-global and survives `gmshClear()`, and three
/// sibling entry points set `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax` and never
/// restore them: `kernel_real::GmshKernel::mesh_to_volume`,
/// `mesh_profile_2d::mesh_plane_2d` and `mesh_boundary`'s surface remesh. Any
/// of those running earlier in the same process pins every element of a later
/// `refine_volume_with_size_field` remesh to *its* size, making the per-vertex
/// `vertex_sizes` hints completely inert (task #6211).
///
/// This test reproduces that leak **deterministically** — it writes the clamp
/// itself via `ffi::option_set_number` rather than depending on which sibling
/// test happened to run first in the binary. That makes it independent of the
/// `mesh_to_volume` pipeline and of sibling task #6200's classify-angle change,
/// and is why it — not the cross-pipeline
/// `uniform_smaller_size_field_produces_more_tets` below — is the primary guard
/// that a smaller size field actually produces a finer mesh.
///
/// Two assertions, both relative (no absolute counts pinned, per the house
/// convention in `mesh_to_volume_tests.rs::mesh_size_override_increases_tet_count`):
///
/// 1. **Monotonicity** — a finer hint yields strictly more tets.
/// 2. **Inbound hermeticity** — each hint is meshed twice, once with the clamp
///    poisoned shut and once with it at gmsh's defaults, and the two runs must
///    agree exactly. This is what makes assertion 1 mean what it says: it
///    pins that the inbound clamp cannot influence the result at all, rather
///    than merely that one particular poisoned run happened to come out
///    monotone.
///
/// The clamp is rewritten before *every* call, because the fixed implementation
/// sets both options on entry; a single up-front write would only exercise the
/// first hint. The poison value is derived from `HINTS` rather than written as
/// a literal — it is the COARSEST hint, i.e. the value that pins the output at
/// the coarsest size the caller asked for, which is the leak's worst case.
///
/// `set_global_mesh_size_clamp` releases `GMSH_LOCK` before
/// `refine_volume_with_size_field` takes it, so a sibling test could otherwise
/// overwrite the clamp in that gap — and because the fix now *resets the clamp
/// to defaults on every exit*, an interleaved sibling refine would erase the
/// poison and turn the poisoned leg into a second defaults run, making
/// assertion 2 hold trivially. That is a false PASS, not a false failure, so
/// it cannot be left to chance: [`CLAMP_TEST_ORDER`] serialises the whole test
/// body against its siblings, making poison → refine atomic. (Asserting the
/// written value is still in place immediately before the call would instead
/// need an `option_get_number` FFI getter, which does not exist and lives
/// outside task #6211's locked files — it is part of task #6212's save/restore
/// discipline.)
#[test]
fn uniform_size_field_refines_monotonically_under_leaked_global_clamp() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    const HINTS: [f64; 3] = [0.5, 0.25, 0.125];
    // The coarsest hint: the worst case for the leak (see docstring).
    const POISON: f64 = HINTS[0];

    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };
    let mut tet_counts: Vec<usize> = Vec::with_capacity(HINTS.len());
    for hint in HINTS {
        let sizes = box_size_field(1.0, |_, _, _| hint);

        // Re-establish the leaked state before each remesh (see docstring).
        poison_global_mesh_size_clamp(POISON);
        let poisoned = refine_tet_count(&cube, &sizes, &opts);

        // Same hints from gmsh's default clamp state: must be identical.
        set_global_mesh_size_clamp(GMSH_CLAMP_DEFAULTS);
        let from_defaults = refine_tet_count(&cube, &sizes, &opts);

        assert_eq!(
            poisoned, from_defaults,
            "the remesh must depend on the size field alone, not on the inbound \
             process-global clamp: hint {hint} gave {poisoned} tets after a \
             Min=Max={POISON} poison but {from_defaults} tets from gmsh's \
             defaults (task #6211)",
        );
        tet_counts.push(poisoned);
    }

    for w in 1..HINTS.len() {
        assert!(
            tet_counts[w] > tet_counts[w - 1],
            "a finer uniform size field must produce strictly more tets: \
             hint {} -> {} tets, hint {} -> {} tets (tet_counts={tet_counts:?}); \
             equal counts mean the per-vertex size field was clamped away by the \
             leaked process-global Mesh.MeshSizeMin/Max (task #6211)",
            HINTS[w - 1],
            tet_counts[w - 1],
            HINTS[w],
            tet_counts[w],
        );
    }
}

/// Edge length of the box the non-uniform test meshes: `[0,SCALE]^3`.
///
/// Deliberately NOT a unit cube. Gmsh's mesher is scale-invariant here, so what
/// matters is `SCALE / COARSE` — how many coarse-hint-sized elements span the
/// domain, i.e. how much interior there is for the mesher to coarsen into.
/// Measured: at `SCALE/COARSE = 2` (the old unit-cube fixture) the boundary
/// triangulation constrains every interior tet and the capped and uncapped runs
/// are indistinguishable to any assertion this test could make; at 4 they
/// separate cleanly. See the "measured" section on the test below.
const SCALE: f64 = 4.0;
/// Hint on the marked half (`x < SPLIT_X`).
const FINE: f64 = 0.25;
/// Hint everywhere else. Also the value of the cap, since
/// `Mesh.MeshSizeMax = size_field.max_size()` and this is the coarsest hint.
const COARSE: f64 = 1.0;
/// The marked/unmarked boundary — the box's mid-plane.
const SPLIT_X: f64 = SCALE / 2.0;
/// How far a realized element may exceed the cap before the test calls it
/// uncapped.
///
/// `Mesh.MeshSizeMax` bounds gmsh's *size field*, not the edge lengths it
/// actually emits; Delaunay insertion overshoots the target where the interior
/// is under-constrained, so some slack is unavoidable and a threshold at
/// exactly `COARSE` would be a false-failure generator. Re-measured under the
/// background size field (task #7447): the largest unmarked element is
/// `1.6230 * COARSE`, leaving `2.0` with ~19% margin.
///
/// The second half of that basis is GONE, and deliberately not papered over:
/// the capped and uncapped readings are now identical (see the measured note
/// on the test below), so this factor no longer straddles a separation. It is
/// a plain overshoot allowance now.
const UNMARKED_MAX_SIZE_SLACK: f64 = 2.0;

/// A NON-uniform size field refines only the marked region and leaves the
/// unmarked region near the coarse hint it asked for.
///
/// This is the production shape — `reify_solver_elastic::volume_refine::
/// refine_with_size_field` always passes a localized field.
///
/// Fine hints on `x < SPLIT_X`, coarse elsewhere. Asserts, all relative:
///
/// 1. the marked half holds strictly more tets than the unmarked half;
/// 2. no single unmarked-half element exceeds
///    `UNMARKED_MAX_SIZE_SLACK * COARSE` — the unmarked region tracks the size
///    it was given rather than coarsening away from it;
/// 3. the marked half's mean element size is strictly smaller than the
///    unmarked half's — localization, not a uniformly-finer mesh.
///
/// # Measured: this assertion no longer pins `Mesh.MeshSizeMax`
///
/// It used to. Under the 0D-corner-anchor path this fixture separated capped
/// from uncapped by 1.9x on the max-edge column (`1.66` vs `3.12` at N=4,
/// widening to `1.89` vs `4.82` at N=6), and assertion 2 was the thing that
/// failed if the cap were reverted to gmsh's default.
///
/// Re-measured under the background size field (task #7447), capped vs
/// `Mesh.MeshSizeMax` forced to `1.0e22`, on a forced rebuild of both legs:
///
/// | leg      | mean edge       | max edge  | counts       |
/// |----------|-----------------|-----------|--------------|
/// | capped   | 0.3711 / 1.1307 | 1.6230    | 5160 / 231   |
/// | uncapped | 0.3711 / 1.1307 | 1.6230    | 5160 / 231   |
///
/// Bit-identical. The cap cannot bind any more, and not merely on this
/// fixture: `Mesh.MeshSizeMax` is `size_field.max_size()`, and a field's values
/// are everywhere `<=` its own maximum, so the cap is redundant WHEREVER the
/// background field is defined. It can only reach the region gmsh extrapolates
/// outside the sizing mesh, which a volume-spanning sizing mesh does not have.
///
/// Two consequences, stated rather than papered over:
///
/// * Assertion 2 still holds and is still worth asserting — the unmarked half
///   tracking its requested size is the property a reader cares about — but it
///   is NO LONGER a guard on the cap. Deleting the `Mesh.MeshSizeMax` write
///   would not fail this test. Task #6211's cap is, on this path, unguarded.
/// * The cap's old cost is gone with its old effect. It used to add unmarked
///   tets (61 -> 85 at N=2, 105 -> 225 at N=4, 150 -> 584 at N=6); it now adds
///   none, because the field already asks for those elements directly.
///
/// The cap is kept as a cheap backstop for the extrapolation region above, not
/// because this fixture demonstrates it doing anything.
///
/// Run under a poisoned clamp for the same reason as the test above.
#[test]
fn non_uniform_size_field_refines_marked_region_and_caps_the_rest() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    let cube = scaled_cube_mesh(SCALE as f32);
    let opts = MeshingOptions {
        mesh_size: Some(COARSE),
        deterministic: true,
        ..Default::default()
    };

    // Fine hint on the x < SPLIT_X side, coarse on the rest.
    let size_field = box_size_field(SCALE, |x, _, _| if x < SPLIT_X { FINE } else { COARSE });
    assert_eq!(
        size_field.max_size(),
        COARSE,
        "fixture must be genuinely non-uniform — the coarsest emitted size sets the cap",
    );

    poison_global_mesh_size_clamp(COARSE);
    let vm = refine_volume_with_size_field(&cube, &size_field, &opts, ElementOrderTag::P1)
        .expect("refine_volume_with_size_field must succeed for a non-uniform field");

    let stats = split_by_centroid_x(&vm, SPLIT_X);
    let (counts, mean_edge, max_edge) = (stats.counts, stats.mean_edge, stats.max_edge);

    assert!(
        counts[0] > 0 && counts[1] > 0,
        "both halves must contain tets, got marked={} unmarked={}",
        counts[0],
        counts[1],
    );
    assert!(
        counts[0] > counts[1],
        "the marked (x<{SPLIT_X}, hint {FINE}) half must hold strictly more tets than the \
         unmarked (hint {COARSE}) half: marked={} unmarked={}, mean edge lengths {mean_edge:?}",
        counts[0],
        counts[1],
    );
    let max_allowed = UNMARKED_MAX_SIZE_SLACK * COARSE;
    assert!(
        max_edge[1] <= max_allowed,
        "the unmarked half's largest element {} must not exceed {max_allowed} \
         ({UNMARKED_MAX_SIZE_SLACK}x the coarsest requested hint {COARSE}): the unmarked \
         region must track the size the field gave it rather than coarsening away from it. \
         Measured {} under the background size field, identical capped and uncapped — see \
         this test's \"no longer pins Mesh.MeshSizeMax\" note before reading a failure here \
         as a cap regression. Mean edge lengths {mean_edge:?}, counts {counts:?}",
        max_edge[1],
        1.6230 * COARSE,
    );
    assert!(
        mean_edge[0] < mean_edge[1],
        "the marked half must be genuinely finer than the unmarked half, got mean edge \
         lengths marked={} unmarked={} — equal values mean the field was applied \
         uniformly rather than locally",
        mean_edge[0],
        mean_edge[1],
    );
}

/// The OUTBOUND half of the #6211 fix: after `refine_volume_with_size_field`
/// returns, a later *defaults-relying* gmsh call must mesh exactly as if the
/// refine had never happened.
///
/// The inbound half — the `Mesh.MeshSizeMin`/`MeshSizeMax` writes on entry — is
/// pinned by the two tests above. The outbound half is `MeshSizeClampReset`,
/// the RAII restore that runs on every exit path. Without a test at this end,
/// deleting that struct and its `let _clamp_reset = …` binding leaves the whole
/// workspace green: the module doc's "leaves nothing" guarantee would be
/// unenforced and free to rot.
///
/// The downstream victim is real, not hypothetical. `mesh_plane_2d(_, _, None,
/// …)` deliberately writes no clamp of its own (`mesh_profile_2d.rs`: the
/// `Mesh.MeshSizeMin/Max` writes sit behind `if let Some(s) = mesh_size`), and
/// `geo_add_point` passes meshSize `0.0` — "no prescribed size here" — so with
/// `Mesh.MeshSizeFromPoints` on and no point sizes, `Mesh.MeshSizeMax` is what
/// decides the element size. A leaked `MeshSizeMax = FINE_HINT` from an
/// adaptive-refinement iteration therefore pins that whole 2D mesh to a size
/// nobody asked for.
///
/// Structure — measure the same defaults-relying call twice, straddling a
/// refine:
///
/// 1. **Warm-up refine.** Not decoration: a refine also writes
///    `Mesh.ElementOrder`, which `mesh_plane_2d` never sets and
///    [`probe_triangle_count`] does not pin. Running one refine first puts it
///    in its post-refine state for BOTH measurements, so the only thing that
///    can differ between them is the clamp — the thing under test.
///    `ElementOrderTag::P1` throughout, so a leaked `Mesh.ElementOrder = 2`
///    (which would make gmsh emit 6-node triangles and the probe's readback
///    return nothing) never arises here.
///
///    It used to carry a second job — normalising the
///    `Mesh.MeshSizeFromPoints` / `FromCurvature` / `ExtendFromBoundary` trio
///    a refine leaks (task #6212, still open) — which the probe now does for
///    itself, unconditionally, so the measurement no longer depends on this
///    warm-up having run.
/// 2. **Baseline**, from an explicitly-defaulted clamp.
/// 3. **A fine refine** — `FINE_HINT` is 20x finer than the plane's own
///    extent, so a leak is loud rather than marginal.
/// 4. **Re-measure.** Must equal the baseline exactly.
///
/// If `MeshSizeClampReset` is removed, step 4 runs under `MeshSizeMax =
/// FINE_HINT` and returns a far denser 2D mesh than step 2, and the equality
/// fails. Needs no `option_get_number` getter: it observes the leak's effect,
/// not the option table.
///
/// # Measured, with `MeshSizeClampReset::armed` commented out of `refine_volume`
///
/// baseline = **162** triangles, after `refine_at(FINE_HINT = 0.05)` = **944** —
/// a 5.8x jump on an assertion that is an exact equality, so the margin is far
/// outside any rounding. The 162 is the same baseline
/// `mesh_to_volume_clamp_hermeticity.rs` measures in its own process, which is
/// the point of sharing [`probe_triangle_count`]: one instrument, one reading,
/// whatever the process has been through.
#[test]
fn refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    /// The hint the refine in the middle requests. 20x finer than the probe's
    /// extent, so a leaked cap changes the probe's triangle count by orders of
    /// magnitude rather than by a rounding.
    const FINE_HINT: f64 = 0.05;

    let cube = unit_cube_mesh();
    let refine_at = |hint: f64| {
        let opts = MeshingOptions {
            mesh_size: Some(hint),
            deterministic: true,
            ..Default::default()
        };
        let size_field = box_size_field(1.0, |_, _, _| hint);
        refine_volume_with_size_field(&cube, &size_field, &opts, ElementOrderTag::P1)
            .unwrap_or_else(|e| panic!("refine_volume_with_size_field({hint}) must succeed: {e:?}"));
    };

    // 1. Warm-up: normalise the #6212-leaked options for both measurements.
    refine_at(0.5);

    // 2. Baseline, from a known-default clamp.
    set_global_mesh_size_clamp(GMSH_CLAMP_DEFAULTS);
    let baseline = probe_triangle_count();
    assert!(
        baseline > 0,
        "the defaults-relying 2D probe must produce triangles; got an empty mesh",
    );

    // 3. A fine refine in between.
    refine_at(FINE_HINT);

    // 4. The same defaults-relying call must be unaffected by it.
    let after_refine = probe_triangle_count();
    assert_eq!(
        after_refine, baseline,
        "refine_volume_with_size_field must restore Mesh.MeshSizeMin/Max to gmsh's defaults \
         on exit: the same mesh_plane_2d(mesh_size: None) call gave {baseline} triangles \
         before a refine at hint {FINE_HINT} and {after_refine} after it. A larger count \
         means the refine's cap leaked outward and pinned an unrelated downstream mesh to \
         a size nobody requested — the outbound direction of task #6211, guarded by \
         `MeshSizeClampReset` in refine_volume.rs",
    );
}

/// A uniform size field smaller than the baseline produces a mesh with
/// strictly more tetrahedra.
///
/// Baseline: unit cube refined under a uniform 0.5 size field. Refinement: the
/// same call under a uniform 0.25 field (half the baseline). The refined volume mesh must have strictly more P1 tets than the
/// baseline, and `element_order` must echo the requested `ElementOrderTag::P1`.
///
/// # Why the baseline is `refine_volume_with_size_field`, not `mesh_to_volume`
///
/// It used to be `GmshKernel::mesh_to_volume(mesh_size = 0.5)`. That control
/// was invalid in two ways, and #6200 exposed it.
///
/// 1. It compared two *different* producers. They do not share sizing
///    semantics: `mesh_to_volume` applies a global target, while this function
///    installs a background size field and lets gmsh read sizes from it.
///    Measured on this cube at the same nominal 0.25, the two
///    disagree by ~2x (mesh_to_volume 382 tets, refine 176), so no inequality
///    between them pins a property of *this* function.
/// 2. The inequality it asserted was an artefact of a bug. Before #6200
///    `mesh_to_volume` passed `classify_surfaces` a 90 deg feature angle and
///    tetrahedralized only part of the solid, so its baseline was tiny: 91 tets
///    at mesh_size 0.5 (aabb fill 0.862916). With the angle fixed the same call
///    returns 194 tets (fill 1.000000) and the assertion inverts against an
///    unchanged refine result. The test was, in effect, pinned to the defect.
///
/// Refining against this function's own coarser output tests the property the
/// name claims — a smaller field yields a denser mesh — with the cross-producer
/// confound removed. Measured: 141 tets at 0.5, 176 at 0.25, both aabb fill
/// 1.000000. This path was never affected by #6200 (it has always classified at
/// PI/12, well below the 90 deg threshold that broke `mesh_to_volume`).
///
/// # What #6211 changed, and why this is still not the size-field guard
///
/// Task #6211 left this test's body untouched but changed *why* the inequality
/// holds. Both calls inherit whatever `Mesh.MeshSizeMin`/`MeshSizeMax` an
/// earlier call left in gmsh's process-global option table, and before #6211
/// this function wrote neither — so a leaked `Min == Max` from a sibling entry
/// point (`GmshKernel::mesh_to_volume`, `mesh_profile_2d::mesh_plane_2d`,
/// `mesh_boundary`'s surface remesh) pinned every element to *that* size and
/// clamped the per-vertex hints away entirely. The inequality could then hold
/// for a reason unrelated to the size field. It now holds because the field is
/// actually honoured.
///
/// That history is exactly why this test cannot be the guard for it: it
/// neither poisons nor reads the clamp, so on unfixed code its outcome depends
/// on what ran before it in the same binary — in a *fresh* process the unfixed
/// code already produces the 141-vs-176 split above and this test passes. The
/// guard is [`uniform_size_field_refines_monotonically_under_leaked_global_clamp`]
/// above, which establishes the leaked clamp itself and is therefore
/// order-independent.
#[test]
fn uniform_smaller_size_field_produces_more_tets() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    // Establish the baseline mesh: same producer, uniform 0.5 hint.
    let baseline_sizes = box_size_field(1.0, |_, _, _| 0.5);
    let vm_baseline =
        refine_volume_with_size_field(&cube, &baseline_sizes, &opts, ElementOrderTag::P1)
            .expect("baseline refine_volume_with_size_field must succeed");

    let n_base_tets = vm_baseline.tet_indices().expect("P1 tet mesh must have tet_indices").len() / 4;
    assert!(n_base_tets > 0, "baseline must have at least one tet");

    // Uniform 0.25 hint: half the baseline hint.
    let vertex_sizes = box_size_field(1.0, |_, _, _| 0.25);

    let result = refine_volume_with_size_field(&cube, &vertex_sizes, &opts, ElementOrderTag::P1);
    let vm_refined = result.expect(
        "refine_volume_with_size_field must succeed for a unit cube with uniform hints",
    );

    assert_eq!(
        vm_refined.element_order(),
        Some(ElementOrderTag::P1),
        "element_order must echo the requested ElementOrderTag::P1",
    );
    let vm_refined_tet_indices = vm_refined.tet_indices().expect("P1 tet mesh must have tet_indices");
    assert_eq!(
        vm_refined_tet_indices.len() % 4,
        0,
        "P1 tet_indices.len() must be divisible by 4, got {}",
        vm_refined_tet_indices.len(),
    );

    let n_refined_tets = vm_refined_tet_indices.len() / 4;
    assert!(
        n_refined_tets > n_base_tets,
        "uniform 0.25 size field must produce strictly more tets than baseline 0.5: \
         baseline={n_base_tets}, refined={n_refined_tets}",
    );
}

/// A refine that asks for many worker threads still returns a mesh.
///
/// # A HANG is this test's red signal, not an assertion message
///
/// If this regresses, the binary stops making progress and the gate's timeout
/// kills it — there is no failure message to go looking for. The cause would be
/// gmsh 4.15.2 deadlocking inside `gmshModelMeshGenerate(3)` while evaluating
/// the `PostView` background size field from several mesher threads at once
/// (task #7447). `refine_volume_with_size_field` defends against that by
/// pinning `General.NumThreads` to 1 unconditionally; the measured thread-count
/// table and the evidence that it is a block rather than slowness live at that
/// option write in `src/refine_volume.rs`.
///
/// # Why 32 threads, and why the count had to be measured here
///
/// The deadlock threshold is FIXTURE-dependent, so a count that hangs another
/// binary proves nothing about this one. Measured on this exact fixture with
/// the pin removed and nothing else changed, each run killed at 120 s:
///
/// | `threads`  | unpinned outcome     |
/// |------------|----------------------|
/// | `Some(8)`  | passes in ~4 s       |
/// | `Some(16)` | hangs                |
/// | `Some(24)` | hangs                |
/// | `Some(32)` | hangs (reproduced 2/2) |
///
/// `Some(8)` is enough to hang `tests/mesher_poison_recovery.rs`, whose gmsh
/// has already been through a `finalize`/`initialize` recovery cycle — but it
/// is NOT enough on a clean process, so writing 8 here would have produced a
/// test that is green on BOTH sides of the fix and guards nothing. `Some(32)`
/// is a literal rather than `available_parallelism` so what gmsh is asked for
/// does not vary with the host's core count; the pin means only one thread is
/// ever actually used, so it costs the gate no CPU.
///
/// Every other test in this file passes `deterministic: true`, i.e. one thread.
/// That is exactly why nothing green caught the deadlock when the background
/// field first landed.
///
/// Deliberately no wall-clock upper bound: a regression manifests as an
/// unbounded hang that the gate timeout already catches, so a duration
/// assertion would add a flake and buy no coverage.
#[test]
fn refine_returns_a_mesh_when_the_caller_asks_for_many_threads() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        threads: Some(32),
        deterministic: false,
    };
    let sizes = box_size_field(1.0, |_, _, _| 0.5);

    let n_tets = refine_tet_count(&cube, &sizes, &opts);
    assert!(
        n_tets > 0,
        "a multi-threaded refine must return a usable volume mesh, got {n_tets} tets",
    );
}
