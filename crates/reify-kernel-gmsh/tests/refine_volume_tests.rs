//! Integration tests for [`reify_kernel_gmsh::refine_volume_with_size_field`].
//!
//! Only compiled and run when `cfg(has_gmsh)` is set by `build.rs` (i.e. when
//! libgmsh was found at build time). On stub builds this file is empty and the
//! test binary contains zero tests, preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::{MeshingOptions, ffi, init, refine_volume_with_size_field};
use reify_ir::{ElementOrderTag, Mesh};

/// Inline copy of `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:19-48`.
///
/// Duplicated rather than dev-dep'ing on `reify-kernel-manifold` to avoid an
/// awkward layering — gmsh would otherwise dev-depend on manifold solely for
/// this 30-line fixture. When B-rep test fixtures consolidate into a shared
/// crate, this helper can move there.
fn unit_cube_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            0.0_f32, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            1.0, 1.0, 0.0, // 2
            0.0, 1.0, 0.0, // 3
            0.0, 0.0, 1.0, // 4
            1.0, 0.0, 1.0, // 5
            1.0, 1.0, 1.0, // 6
            0.0, 1.0, 1.0, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            // -Z bottom (outward = -Z, CW from +Z view)
            0, 2, 1,  0, 3, 2,
            // +Z top
            4, 5, 6,  4, 6, 7,
            // -Y front
            0, 1, 5,  0, 5, 4,
            // +Y back
            3, 7, 6,  3, 6, 2,
            // -X left
            0, 4, 7,  0, 7, 3,
            // +X right
            1, 2, 6,  1, 6, 5,
        ],
        normals: None,
    }
}

/// Gmsh's documented defaults for the `Mesh.MeshSizeMin`/`MeshSizeMax` pair —
/// no floor, effectively no cap. This is the state a caller that writes no
/// clamp of its own (e.g. `mesh_plane_2d` with no requested size) expects.
const GMSH_CLAMP_DEFAULTS: (f64, f64) = (0.0, 1.0e22);

/// Write the process-global gmsh mesh-size clamp.
///
/// gmsh's option table is process-global and is **not** reset by `gmshClear()`,
/// so `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax` written by one call survive into
/// every later call in the same process. Acquires `GMSH_LOCK` for the duration
/// of the two writes and releases it before returning, so the subsequent
/// `refine_volume_with_size_field` call can take the lock itself.
fn set_global_mesh_size_clamp((min, max): (f64, f64)) {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    ffi::option_set_number("Mesh.MeshSizeMin", min).expect("set MeshSizeMin");
    ffi::option_set_number("Mesh.MeshSizeMax", max).expect("set MeshSizeMax");
}

/// Pin the clamp shut at `size`, reproducing the state a sibling entry point
/// leaves behind (`Min == Max == its own requested size`).
fn poison_global_mesh_size_clamp(size: f64) {
    set_global_mesh_size_clamp((size, size));
}

/// Remesh `cube` with the given per-vertex hints and return the P1 tet count.
fn refine_tet_count(cube: &Mesh, vertex_sizes: &[f64], opts: &MeshingOptions) -> usize {
    let vm = refine_volume_with_size_field(cube, vertex_sizes, opts, ElementOrderTag::P1)
        .unwrap_or_else(|e| panic!("refine_volume_with_size_field must succeed: {e:?}"));
    vm.tet_indices().expect("P1 tet mesh").len() / 4
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
/// Note the residual fragility this cannot close: `set_global_mesh_size_clamp`
/// releases `GMSH_LOCK` before `refine_volume_with_size_field` takes it, so a
/// test running in parallel could overwrite the clamp in between. Asserting
/// the written value is still in place immediately before the call would need
/// an `option_get_number` FFI getter, which does not exist and lives outside
/// task #6211's locked files — it is part of task #6212's save/restore
/// discipline. Assertion 2 is the practical substitute: an interleave that
/// mattered would break the two runs' equality rather than pass quietly.
#[test]
fn uniform_size_field_refines_monotonically_under_leaked_global_clamp() {
    const HINTS: [f64; 3] = [0.5, 0.25, 0.125];
    // The coarsest hint: the worst case for the leak (see docstring).
    const POISON: f64 = HINTS[0];

    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };
    let n_surface_verts = cube.vertices.len() / 3;

    let mut tet_counts: Vec<usize> = Vec::with_capacity(HINTS.len());
    for hint in HINTS {
        let sizes = vec![hint; n_surface_verts];

        // Re-establish the leaked state before each remesh (see docstring).
        poison_global_mesh_size_clamp(POISON);
        let poisoned = refine_tet_count(&cube, &sizes, &opts);

        // Same hints from gmsh's default clamp state: must be identical.
        set_global_mesh_size_clamp(GMSH_CLAMP_DEFAULTS);
        let from_defaults = refine_tet_count(&cube, &sizes, &opts);

        assert_eq!(
            poisoned, from_defaults,
            "the remesh must depend on `vertex_sizes` alone, not on the inbound \
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

/// A NON-uniform size field refines only the marked region, and the cap the fix
/// introduces (`Mesh.MeshSizeMax = max(vertex_sizes)`) keeps the unmarked
/// region at roughly the coarse hint rather than letting it grow arbitrarily.
///
/// This is the production shape — `reify_solver_elastic::volume_refine::
/// refine_with_size_field` always passes a localized field — and it is the case
/// where the `max_hint` cap actually changes behaviour: with
/// `Mesh.MeshSizeExtendFromBoundary = 0` the 3D mesher is otherwise free to
/// coarsen the interior past anything the caller asked for. The uniform test
/// above cannot see that, because there the cap coincides with the single hint.
///
/// Fine hints on `x < 0.5`, coarse elsewhere. Asserts, all relative:
///
/// 1. the marked half holds strictly more tets than the unmarked half;
/// 2. the unmarked half's mean element size does not exceed the coarsest
///    requested hint — the contract `Mesh.MeshSizeMax = max(vertex_sizes)`
///    exists to state;
/// 3. the marked half's mean element size is strictly smaller than the
///    unmarked half's — localization, not a uniformly-finer mesh.
///
/// # What assertion 2 does and does not catch, measured
///
/// Re-running this fixture with `MeshSizeMax` forced to gmsh's uncapped
/// default gives `[marked, unmarked]` tet counts `[177, 61]` and mean edge
/// lengths `[0.3009, 0.4414]`, against `[200, 85]` / `[0.2857, 0.3903]` with
/// the cap in place. So the cap does measurably tighten the unmarked region
/// (~13% finer, 85 tets vs 61) — but on a UNIT cube the interior has no room
/// to coarsen past the 0.5 hint anyway, so the uncapped mean edge length
/// (0.4414) also satisfies assertion 2. Assertion 2 therefore pins the
/// contract, not the mechanism: it would fail on a geometry with enough
/// interior to coarsen, and it fails here if anything else lets the unmarked
/// region drift past what the caller asked for, but on this fixture it does
/// not by itself detect removal of the cap. Deliberately NOT tightened to a
/// threshold between 0.3903 and 0.4414: a ~7% band either side of a
/// mesher-version-sensitive quantity is a false-failure generator, and the
/// cap's real justification is the cross-pipeline margin documented on
/// `refine_volume_with_size_field`'s option writes.
///
/// Run under a poisoned clamp for the same reason as the test above.
#[test]
fn non_uniform_size_field_refines_marked_region_and_caps_the_rest() {
    const FINE: f64 = 0.125;
    const COARSE: f64 = 0.5;

    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(COARSE),
        deterministic: true,
        ..Default::default()
    };

    // Fine hint on the x < 0.5 face, coarse on the rest.
    let vertex_sizes: Vec<f64> = cube
        .vertices
        .chunks_exact(3)
        .map(|xyz| if (xyz[0] as f64) < 0.5 { FINE } else { COARSE })
        .collect();
    assert!(
        vertex_sizes.contains(&FINE) && vertex_sizes.contains(&COARSE),
        "fixture must produce a genuinely non-uniform field, got {vertex_sizes:?}",
    );

    poison_global_mesh_size_clamp(COARSE);
    let vm = refine_volume_with_size_field(&cube, &vertex_sizes, &opts, ElementOrderTag::P1)
        .expect("refine_volume_with_size_field must succeed for a non-uniform field");

    let verts = &vm.vertices;
    let tets = vm.tet_indices().expect("P1 tet mesh");

    // The six edges of a tet, as index pairs into its four corners.
    const TET_EDGES: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

    // Split tets by centroid side, accumulating mean edge length per side.
    let mut counts = [0_usize; 2]; // [marked (x<0.5), unmarked]
    let mut edge_len_sums = [0.0_f64; 2];
    for tet in tets.chunks_exact(4) {
        let p: [[f64; 3]; 4] = std::array::from_fn(|k| {
            let b = 3 * tet[k] as usize;
            [verts[b] as f64, verts[b + 1] as f64, verts[b + 2] as f64]
        });
        let centroid_x = p.iter().map(|q| q[0]).sum::<f64>() / 4.0;
        let side = usize::from(centroid_x >= 0.5);

        let mut edge_sum = 0.0;
        for (a, b) in TET_EDGES {
            let d = [p[a][0] - p[b][0], p[a][1] - p[b][1], p[a][2] - p[b][2]];
            edge_sum += (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        }
        counts[side] += 1;
        edge_len_sums[side] += edge_sum / TET_EDGES.len() as f64;
    }

    assert!(
        counts[0] > 0 && counts[1] > 0,
        "both halves must contain tets, got marked={} unmarked={}",
        counts[0],
        counts[1],
    );
    let mean_edge = [
        edge_len_sums[0] / counts[0] as f64,
        edge_len_sums[1] / counts[1] as f64,
    ];

    assert!(
        counts[0] > counts[1],
        "the marked (x<0.5, hint {FINE}) half must hold strictly more tets than the \
         unmarked (hint {COARSE}) half: marked={} unmarked={}, mean edge lengths {mean_edge:?}",
        counts[0],
        counts[1],
    );
    assert!(
        mean_edge[1] <= COARSE,
        "the unmarked half's mean element size {} must not exceed the coarsest requested \
         hint {COARSE} — that is the contract Mesh.MeshSizeMax = max(vertex_sizes) states; \
         a larger value means the interior coarsened past what the caller asked for \
         (task #6211). Measured margin on this fixture: 0.3903 capped, 0.4414 uncapped",
        mean_edge[1],
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

/// A uniform size field smaller than the baseline produces a mesh with
/// strictly more tetrahedra.
///
/// Baseline: unit cube meshed at target size 0.5 (via `GmshKernel::mesh_to_volume`).
/// Refinement: call `refine_volume_with_size_field` with every surface vertex
/// assigned size 0.25 (half the baseline target). The refined volume mesh must
/// have strictly more P1 tets than the baseline, and `element_order` must echo
/// the requested `ElementOrderTag::P1`.
///
/// This comparison is CROSS-PIPELINE: the baseline comes from
/// `GmshKernel::mesh_to_volume` (which classifies at `FRAC_PI_2`) while the
/// refined mesh comes from `refine_volume_with_size_field` (which classifies at
/// `PI/12`). It is therefore a sanity check that the two paths agree in
/// direction — NOT the primary size-field guard. That role belongs to
/// [`uniform_size_field_refines_monotonically_under_leaked_global_clamp`] above,
/// which is pipeline-independent and is what actually pins "a smaller size field
/// produces a finer mesh".
///
/// The assertion is purely relative, so it carries no calibration to go stale.
/// What #6211 changed is *why* it holds: before the fix the refined mesh was
/// clamped to whatever size a sibling entry point had leaked, which the
/// baseline happened to under-resolve — so the inequality passed while the
/// size field was entirely inert. It now holds because the size field is
/// actually honoured.
#[test]
fn uniform_smaller_size_field_produces_more_tets() {
    use reify_kernel_gmsh::GmshKernel;

    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    // Establish the baseline mesh.
    let vm_baseline = kernel
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .expect("baseline mesh_to_volume must succeed");

    let n_base_tets = vm_baseline.tet_indices().expect("P1 tet mesh must have tet_indices").len() / 4;
    assert!(n_base_tets > 0, "baseline must have at least one tet");

    let n_surface_verts = cube.vertices.len() / 3;
    assert!(
        n_surface_verts > 0,
        "unit cube must have at least one surface vertex"
    );

    // Uniform 0.25 per-vertex hint: half the baseline target.
    let vertex_sizes = vec![0.25_f64; n_surface_verts];

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
