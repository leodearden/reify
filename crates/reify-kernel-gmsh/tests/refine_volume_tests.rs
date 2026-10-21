//! Integration tests for [`reify_kernel_gmsh::refine_volume_with_size_field`].
//!
//! Only compiled and run when `cfg(has_gmsh)` is set by `build.rs` (i.e. when
//! libgmsh was found at build time). On stub builds this file is empty and the
//! test binary contains zero tests, preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

// The clamp probe and its serialising mutex are shared verbatim with
// `tests/mesh_size_option_hermeticity.rs`, the other half of this
// discipline. Declared by `#[path]`; see `common/clamp_probe.rs` for why, and
// why one copy matters.
#[path = "common/clamp_probe.rs"]
mod clamp_probe;

use clamp_probe::{
    CLAMP_TEST_ORDER, GMSH_CLAMP_DEFAULTS, assert_all_size_options_at_gmsh_defaults,
    poison_global_mesh_size_clamp, probe_triangle_count, set_global_mesh_size_clamp,
};
use reify_ir::{ElementOrderTag, Mesh, VolumeConnectivity, VolumeMesh};
use reify_kernel_gmsh::{
    BackgroundSizeField, MeshingOptions, ffi, init, refine_volume_with_size_field,
};
use reify_test_support::mesh_fixtures::unit_cube_mesh;

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
/// sibling entry points USED TO set `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax`
/// and restore neither: `kernel_real::GmshKernel::mesh_to_volume`,
/// `mesh_profile_2d::mesh_plane_2d` and `mesh_boundary`'s surface remesh. Any
/// of those running earlier in the same process pinned every element of a later
/// `refine_volume_with_size_field` remesh to *its* size, making the per-vertex
/// `vertex_sizes` hints completely inert (task #6211). All three now enter and
/// leave a `mesh_size_scope::MeshSizeScope` (#6968), so the leak is closed at
/// its source; this test keeps reproducing it SYNTHETICALLY, by writing the
/// hostile table itself, which is what makes it independent of whether any
/// sibling is still capable of producing one.
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
/// # What assertion 2 pins TODAY, which is not what it was written to pin
///
/// It was written against task #6211, when the thing standing between the
/// poison and the mesher was `refine_volume_with_size_field`'s own
/// `Mesh.MeshSizeMin`/`MeshSizeMax` writes. Since #6968 the poison is erased
/// before those are reached: `MeshSizeScope::entered` (`refine_volume.rs`,
/// immediately after `init::ensure_initialized()`) restores gmsh's defaults
/// for all five size options on the way in. So assertion 2 now pins the
/// SCOPE's inbound establishment, and with the scope armed the two legs are
/// two identical defaults runs — green by construction.
///
/// That is not a reason to delete it. It is the only test here that reds if
/// the inbound establishment disappears by ANY route, and it observes the
/// EFFECT (a tet count) rather than reading the table, so it cannot be
/// defeated by a leak through an option no test thought to name.
///
/// A guard for refine's OWN inbound writes specifically is not reachable from
/// a test: it would have to poison the table AFTER the scope has entered and
/// BEFORE the clamp writes run, and that is inside one `GMSH_LOCK`
/// acquisition, with no seam. Said plainly rather than left as an assertion
/// this test looks like it makes — `refine_volume.rs`'s own "NO TEST CAN TELL"
/// note is the other half of it.
///
/// The clamp is rewritten before *every* call, because the fixed implementation
/// sets both options on entry; a single up-front write would only exercise the
/// first hint. The poison value is derived from `HINTS` rather than written as
/// a literal — it is the COARSEST hint, i.e. the value that pins the output at
/// the coarsest size the caller asked for, which is the leak's worst case.
///
/// [`CLAMP_TEST_ORDER`] is still taken across the whole body, and what it buys
/// is now the test's FALSIFIABILITY rather than today's correctness.
/// `set_global_mesh_size_clamp` releases `GMSH_LOCK` before
/// `refine_volume_with_size_field` takes it. On a build where the inbound
/// establishment is gone — the build this test exists to catch — a sibling
/// landing in that gap would erase the poison and turn the poisoned leg into a
/// second defaults run, so the test would pass exactly when it should fail.
/// That is a false PASS, the worse direction for a regression guard, and the
/// mutex makes poison → refine atomic against its siblings. (Reading the
/// written value back immediately before the call, via the `option_get_number`
/// task #6968 added, would not substitute: it reports the table at the moment
/// it is called, not across the gap a sibling can land in.)
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
/// domain, i.e. how much interior there is for the mesher to coarsen into, which
/// is what lets the non-uniform test's no-coarsening assertion fail at all. At
/// `SCALE/COARSE = 2` (the old unit-cube fixture) the boundary triangulation
/// constrained every interior tet.
const SCALE: f64 = 4.0;
/// Hint on the marked half (`x < SPLIT_X`).
const FINE: f64 = 0.25;
/// Hint everywhere else. Also the value of the cap, since
/// `Mesh.MeshSizeMax = size_field.max_size()` and this is the coarsest hint.
const COARSE: f64 = 1.0;
/// The marked/unmarked boundary — the box's mid-plane.
const SPLIT_X: f64 = SCALE / 2.0;
/// How far a realized unmarked element may exceed `COARSE` before the test
/// calls the unmarked region coarsened.
///
/// A size field sets the size gmsh *targets*, not the edge lengths it emits;
/// Delaunay insertion overshoots the target where the interior is
/// under-constrained, so some slack is unavoidable and a threshold at exactly
/// `COARSE` would be a false-failure generator. Measured under the background
/// size field (task #7447): the largest unmarked element is `1.6230 * COARSE`,
/// leaving `2.0` with ~19% margin.
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
/// # Assertion 2 is about the field, not about `Mesh.MeshSizeMax`
///
/// It used to guard the cap. Under the 0D-corner-anchor path this fixture
/// separated capped from uncapped by 1.9x on the max-edge column (`1.66` vs
/// `3.12` at N=4), and assertion 2 failed if the cap were reverted to gmsh's
/// default. Re-measured under the background size field (task #7447), capped vs
/// `Mesh.MeshSizeMax` forced to `1.0e22`:
///
/// | leg      | mean edge       | max edge  | counts       |
/// |----------|-----------------|-----------|--------------|
/// | capped   | 0.3711 / 1.1307 | 1.6230    | 5160 / 231   |
/// | uncapped | 0.3711 / 1.1307 | 1.6230    | 5160 / 231   |
///
/// Bit-identical: the field itself now asks for the unmarked elements, so the
/// cap has nothing left to bound here. Why it cannot bind anywhere — outside
/// the sizing mesh included — is stated once, at the `Mesh.MeshSizeMax` write
/// in `refine_volume.rs`; the region outside the sizing mesh is pinned by
/// [`the_region_outside_a_partial_sizing_mesh_takes_the_nearest_hint_not_the_cap`].
///
/// Run under a poisoned clamp for the same reason as the test above.
#[test]
fn non_uniform_size_field_refines_marked_region_and_does_not_coarsen_the_rest() {
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
         Measured {} under the background size field, identical capped and uncapped — a \
         failure here is a sizing regression, not a cap regression. Mean edge lengths \
         {mean_edge:?}, counts {counts:?}",
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

/// Edge length of the sub-box `[0, SIZING_SPAN]^3` the partial sizing mesh
/// covers: one eighth of the `[0, SCALE]^3` box it sizes.
const SIZING_SPAN: f64 = SCALE / 2.0;
/// Lower bound, in centroid `x`, of the band the partial-sizing-mesh test
/// reads. Every tet there lies at least `SCALE / 4` beyond the sizing mesh.
const OUTSIDE_BAND_MIN_X: f64 = 3.0 * SCALE / 4.0;
/// How far the outside band's mean edge may stray from the nearest-hint
/// reference, as a fraction of it: the ±25% the solver-elastic localization
/// test uses for its far band.
const NEAREST_HINT_TOLERANCE: f64 = 0.25;

/// Outside the region its sizing mesh covers, the background field takes the
/// NEAREST node's hint. It does not fall back to the coarsest hint, which is
/// the value of the `Mesh.MeshSizeMax` cap.
///
/// The sizing mesh covers only `[0, SIZING_SPAN]^3`, and its hints are laid out
/// so the nearest one and the coarsest one disagree: `COARSE` on
/// `x < SIZING_SPAN / 2`, `FINE` on the rest, so its face nearest the band read
/// here carries `FINE` while `max_size()`, and so the cap, is `COARSE`. The band
/// `x >= OUTSIDE_BAND_MIN_X` lies wholly outside the sizing mesh.
///
/// Its mean tet edge is compared with the same band remeshed under two
/// whole-box uniform references, `FINE` and `COARSE`. It must sit within
/// `NEAREST_HINT_TOLERANCE` of the `FINE` one, and the test first asserts that
/// the `COARSE` one falls outside that window, so this outcome cannot be
/// confused with the cap's. Measured (task #7447, libgmsh 4.15.2):
///
/// | remesh                                  | outside band mean edge (tets) |
/// |-----------------------------------------|-------------------------------|
/// | partial sizing mesh                     | 0.3380 (4440)                 |
/// | uniform `FINE` reference                | 0.3377 (4437)                 |
/// | uniform `COARSE` reference              | 1.1247 (111)                  |
/// | partial, field's `UseClosest` forced 0  | 1.1471 — this test fails      |
///
/// The partial reading is identical with `Mesh.MeshSizeMax` forced to
/// `1.0e22`: the cap does not bind here either. That is gmsh's `PostView`
/// field extending its view by the nearest node (its `UseClosest` option,
/// which `refine_volume_with_size_field` leaves at gmsh's default), and it is
/// half of why the cap can never bind. The last row is the mutation check:
/// with the extension off, the band falls to the cap and reads next to the
/// `COARSE` reference. The whole argument is stated once, at the
/// `Mesh.MeshSizeMax` write in `refine_volume.rs`; this test is what goes red
/// if a gmsh upgrade stops extending the view that way.
#[test]
fn the_region_outside_a_partial_sizing_mesh_takes_the_nearest_hint_not_the_cap() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    let cube = scaled_cube_mesh(SCALE as f32);
    let opts = MeshingOptions {
        mesh_size: Some(COARSE),
        deterministic: true,
        ..Default::default()
    };
    let outside_band_mean_edge = |size_field: &BackgroundSizeField, what: &str| -> f64 {
        let vm = refine_volume_with_size_field(&cube, size_field, &opts, ElementOrderTag::P1)
            .unwrap_or_else(|e| {
                panic!("{what}: refine_volume_with_size_field must succeed: {e:?}")
            });
        let stats = split_by_centroid_x(&vm, OUTSIDE_BAND_MIN_X);
        assert!(
            stats.counts[1] > 0,
            "{what}: the band x >= {OUTSIDE_BAND_MIN_X} must contain tets, counts {:?}",
            stats.counts,
        );
        stats.mean_edge[1]
    };

    let partial = box_size_field(SIZING_SPAN, |x, _, _| {
        if x < SIZING_SPAN / 2.0 { COARSE } else { FINE }
    });
    assert_eq!(
        partial.max_size(),
        COARSE,
        "fixture: the coarsest hint, and so the cap, must be COARSE, away from the band",
    );

    let fine_reference = outside_band_mean_edge(&box_size_field(SCALE, |_, _, _| FINE), "FINE");
    let coarse_reference =
        outside_band_mean_edge(&box_size_field(SCALE, |_, _, _| COARSE), "COARSE");
    let window = (1.0 - NEAREST_HINT_TOLERANCE) * fine_reference
        ..=(1.0 + NEAREST_HINT_TOLERANCE) * fine_reference;
    assert!(
        !window.contains(&coarse_reference),
        "fixture: the uniform COARSE reference ({coarse_reference}) must fall outside the \
         nearest-hint window {window:?}, or this test cannot tell the nearest hint from the cap",
    );

    let observed = outside_band_mean_edge(&partial, "partial sizing mesh");
    assert!(
        window.contains(&observed),
        "outside the sizing mesh the remesh must follow the NEAREST hint ({FINE}): band \
         x >= {OUTSIDE_BAND_MIN_X} mean edge {observed}, window {window:?} around the uniform \
         {FINE} reference {fine_reference}; the uniform {COARSE} reference reads \
         {coarse_reference}. A reading near that means gmsh stopped extending the view by its \
         nearest node, and the cap now decides these sizes: revisit the Mesh.MeshSizeMax note \
         in refine_volume.rs (measured 0.3380 against a 0.3377 reference, task #7447)",
    );
}

/// The OUTBOUND half of the #6211 fix: after `refine_volume_with_size_field`
/// returns, a later *defaults-relying* gmsh call must mesh exactly as if the
/// refine had never happened.
///
/// The inbound half — the `Mesh.MeshSizeMin`/`MeshSizeMax` writes on entry — is
/// pinned by the two tests above. The outbound half is `MeshSizeScope`, the
/// RAII scope that restores every size option on every exit path. Without a
/// test at this end, deleting that type and its `let _size_scope = …` binding
/// leaves the whole workspace green: the module doc's "leaves nothing"
/// guarantee would be unenforced and free to rot.
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
///    a refine leaked. Task #6968 closed that leak at the source, so a refine
///    now restores the trio itself and neither this warm-up nor the probe has
///    to compensate for it.
/// 2. **Baseline**, from an explicitly-defaulted clamp.
/// 3. **A fine refine** — `FINE_HINT` is 20x finer than the plane's own
///    extent, so a leak is loud rather than marginal.
/// 4. **Re-measure.** Must equal the baseline exactly.
///
/// If `MeshSizeScope` is removed, step 4 runs under `MeshSizeMax =
/// FINE_HINT` and returns a far denser 2D mesh than step 2, and the equality
/// Observes the leak's EFFECT rather than the option table, which is the
/// stronger half of the pair: it fails on a leak by any route, not only via an
/// option name a test thought to read. The complementary direct table read is
/// [`refine_volume_leaves_every_size_option_at_gmsh_defaults`].
///
/// # Measured, with `MeshSizeScope::entered` commented out of `refine_volume`
///
/// baseline = **162** triangles, after `refine_at(FINE_HINT = 0.05)` = **944** —
/// a 5.8x jump on an assertion that is an exact equality, so the margin is far
/// outside any rounding. The 162 is the same baseline
/// `mesh_size_option_hermeticity.rs` measures in its own process, which is
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

    // 1. Warm-up: put Mesh.ElementOrder in its post-refine state for both
    //    measurements. See the doc comment above.
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
         `MeshSizeScope` in refine_volume.rs",
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
/// This is the hang guard for the `PostView` octree warm-up in
/// `refine_volume.rs`'s `BackgroundFieldGuard::install`, and a HANG is its red
/// signal, not an assertion message: without the warm-up gmsh 4.15.2
/// deadlocks inside `gmshModelMeshGenerate(3)` and the gate's timeout kills the
/// binary.
///
/// `Some(32)` is a literal above this fixture's 14 classified curves, the count
/// the deadlock tracks (measured without the warm-up: 14 threads pass, 15
/// hang), so what gmsh is asked for does not vary with the host's core count.
/// Mechanism and measurements: `docs/notes/gmsh-postview-background-field-threading.md`.
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

/// `refine_volume_with_size_field` hands gmsh the caller's resolved thread
/// count, read back from gmsh's option table after the call. The read is valid
/// because `General.NumThreads` is a non-size process-global no scope restores:
/// refine writes it and leaves it, exactly like `mesh_to_volume`. If a later
/// task scopes `General.NumThreads`, this read must move inside that scope.
///
/// 3 threads is below the cube's 14 classified curves, so this test reds by
/// assertion, never by hang.
#[test]
fn refine_hands_gmsh_the_callers_resolved_thread_count() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    let cube = unit_cube_mesh();
    let sizes = box_size_field(1.0, |_, _, _| 0.5);
    let cases = [
        (
            MeshingOptions {
                mesh_size: Some(0.5),
                threads: Some(3),
                deterministic: false,
            },
            3.0,
        ),
        (
            MeshingOptions {
                mesh_size: Some(0.5),
                threads: Some(3),
                deterministic: true,
            },
            1.0,
        ),
    ];

    for (opts, expected_threads) in cases {
        refine_volume_with_size_field(&cube, &sizes, &opts, ElementOrderTag::P1)
            .unwrap_or_else(|e| panic!("refine_volume_with_size_field must succeed: {e:?}"));
        let handed_to_gmsh = {
            let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            ffi::option_get_number("General.NumThreads")
                .expect("ffi::option_get_number(General.NumThreads) failed")
        };
        assert_eq!(
            handed_to_gmsh, expected_threads,
            "for {opts:?} gmsh must be asked for {expected_threads} threads",
        );
    }
}

/// `refine_volume_with_size_field` leaves EVERY mesh-size process-global at
/// gmsh's default, not just the `Mesh.MeshSizeMin`/`MeshSizeMax` pair.
///
/// The sibling guard above
/// ([`refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`])
/// covers the pair by observing its effect on a defaults-relying probe. This
/// one reads the option table directly, via the `ffi::option_get_number` added
/// by #6968, and so covers the three size-SOURCE options a density probe
/// cannot reach: their effect is invisible whenever `MeshSizeMin ==
/// MeshSizeMax`, which is every reachable `mesh_to_volume` path.
///
/// The read-back itself is [`clamp_probe::assert_all_size_options_at_gmsh_defaults`],
/// shared with the other three per-entry-point guards; it iterates the
/// production `mesh_size_scope::GMSH_SIZE_OPTION_DEFAULTS` rather than naming
/// options, which is what makes this a guard for the SEAM rather than for
/// today's five: a sixth process-global added to the production list is
/// asserted by all four writers on the day it is added, with no test edit.
///
/// # Measured leak this closes
///
/// Run against the pre-#6968 tree, with the production code otherwise
/// untouched, an in-process probe of the same shape read back, after one
/// `refine_volume_with_size_field(unit_cube_mesh(), hint 0.5, P1)`:
///
/// ```text
/// Mesh.MeshSizeMin                = 0      (default 0)      OK
/// Mesh.MeshSizeMax                = 1e22   (default 1e22)   OK
/// Mesh.MeshSizeFromPoints         = 1      (default 1)      OK
/// Mesh.MeshSizeFromCurvature      = 0      (default 0)      OK
/// Mesh.MeshSizeExtendFromBoundary = 0      (default 1)      *** LEAK ***
/// ```
///
/// On that tree exactly one of the three trio writes at `refine_volume.rs`
/// deviated from a gmsh default: `MeshSizeFromPoints = 1` and
/// `MeshSizeFromCurvature = 0` restated defaults and were no-ops, while
/// `MeshSizeExtendFromBoundary = 0` against a default of `1` was the whole
/// leak. The guard covered all five anyway, because "correct today, silently
/// wrong the first time someone changes one of the other two" is precisely the
/// failure mode #6968 exists to close — and task #7447 then did change one:
/// with sizing moved onto a background field, refine now writes
/// `MeshSizeFromPoints = 0`, so TWO of the three deviate today, and a leaked
/// `0` would disable point-driven sizing for every later call in the process.
/// This test is the behavioural guard for both.
#[test]
fn refine_volume_leaves_every_size_option_at_gmsh_defaults() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };
    let sizes = box_size_field(1.0, |_, _, _| 0.5);
    refine_volume_with_size_field(&cube, &sizes, &opts, ElementOrderTag::P1)
        .unwrap_or_else(|e| panic!("refine_volume_with_size_field must succeed: {e:?}"));

    assert_all_size_options_at_gmsh_defaults(
        "refine_volume_with_size_field",
        "`MeshSizeScope` in refine_volume.rs",
    );
}
