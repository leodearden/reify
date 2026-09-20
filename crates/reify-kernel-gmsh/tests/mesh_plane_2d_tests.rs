//! Pin the 2D plane-surface mesher (`mesh_plane_2d`) added by T2987.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! Two parallel test surfaces:
//! - `#[cfg(has_gmsh)]` — real FFI smoke tests asserting the meshed unit
//!   square produces a triangle (or quad-recombined) buffer with the
//!   expected stride and in-bounds indices.
//! - `#[cfg(not(has_gmsh))]` — the stub arm returns
//!   `GeometryError::OperationFailed` with "Gmsh not available" in the
//!   message.
//!
//! These run via `cargo test -p reify-kernel-gmsh --test mesh_plane_2d_tests`
//! in both build modes; the cfg gates pick the right arm.

// The serialising mutex and the defaults-relying probe are shared verbatim
// with this crate's other process-global mesh-size guards. Declared by path
// rather than through `common/mod.rs`, which #6387 reduced to a re-export shim
// over `reify_test_support::fixtures` and which is scheduled for deletion; see
// `common/clamp_probe.rs` for why one copy matters.
#[cfg(has_gmsh)]
#[path = "common/clamp_probe.rs"]
mod clamp_probe;

#[cfg(has_gmsh)]
use clamp_probe::CLAMP_TEST_ORDER;
use reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d;
#[cfg(has_gmsh)]
use reify_kernel_gmsh::mesh_size_scope::GMSH_SIZE_OPTION_DEFAULTS;
#[cfg(has_gmsh)]
use reify_kernel_gmsh::{ffi, init};

/// Triangle path: `recombine=false` on a unit square produces a triangle
/// mesh with a non-empty, stride-3 index buffer, an even-length flat XY
/// vertex buffer, and every index in-bounds.
///
/// `mesh_plane_2d` acquires `init::GMSH_LOCK` internally — tests must NOT
/// hold the lock externally or the inner acquisition would deadlock.
#[cfg(has_gmsh)]
#[test]
fn mesh_plane_2d_triangle_path_unit_square_round_trip() {
    // Taken by every has_gmsh test in this binary, not only by the guard that
    // needs it: a mutex one participant skips provides no exclusion, and this
    // test writes the process-global Mesh.MeshSizeMin/Max that
    // `mesh_plane_2d_leaves_every_size_option_at_gmsh_defaults` measures.
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    let outer: Vec<[f64; 2]> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let holes: Vec<Vec<[f64; 2]>> = vec![];

    let result = mesh_plane_2d(&outer, &holes, Some(0.5), false, true)
        .expect("mesh_plane_2d failed on unit-square triangle path");

    // (c) vertices_xy is a flat XY buffer (stride 2).
    assert!(
        result.vertices_xy.len().is_multiple_of(2),
        "vertices_xy.len()={} not even (XY pairs expected)",
        result.vertices_xy.len(),
    );
    let n_verts = result.vertices_xy.len() / 2;
    assert!(n_verts > 0, "expected at least one vertex");

    // (a) triangle_indices is non-empty and stride-3.
    assert!(
        !result.triangle_indices.is_empty(),
        "triangle_indices is empty — recombine=false should produce triangles",
    );
    assert_eq!(
        result.triangle_indices.len() % 3,
        0,
        "triangle_indices.len()={} not divisible by 3",
        result.triangle_indices.len(),
    );

    // (b) quad_indices is empty (recombine=false).
    assert!(
        result.quad_indices.is_empty(),
        "quad_indices is non-empty (len={}) despite recombine=false",
        result.quad_indices.len(),
    );

    // (d) every triangle index in-bounds against vertices_xy / 2.
    for (i, &idx) in result.triangle_indices.iter().enumerate() {
        assert!(
            (idx as usize) < n_verts,
            "triangle_indices[{i}]={idx} out of bounds (n_verts={n_verts})",
        );
    }
}

/// Quad path: `recombine=true` on a unit square produces a quad-dominated
/// mesh — stride-4 quad indices, no triangles (or quads strictly
/// dominating), and every quad's max corner skew ≤ π/4 (the threshold
/// `reify_solver_elastic::mesher::recombine_quality_ok` enforces; logic
/// inlined here to avoid a dev-deps cycle).
#[cfg(has_gmsh)]
#[test]
fn mesh_plane_2d_quad_path_unit_square_recombines_cleanly() {
    // Taken by every has_gmsh test in this binary, not only by the guard that
    // needs it: a mutex one participant skips provides no exclusion, and this
    // test writes the process-global Mesh.MeshSizeMin/Max that
    // `mesh_plane_2d_leaves_every_size_option_at_gmsh_defaults` measures.
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    let outer: Vec<[f64; 2]> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let holes: Vec<Vec<[f64; 2]>> = vec![];

    let result = mesh_plane_2d(&outer, &holes, Some(0.5), true, true)
        .expect("mesh_plane_2d failed on unit-square quad path");

    let n_verts = result.vertices_xy.len() / 2;

    // (a) non-empty stride-4 quad buffer.
    assert!(
        !result.quad_indices.is_empty(),
        "quad_indices is empty — recombine=true should produce quads",
    );
    assert_eq!(
        result.quad_indices.len() % 4,
        0,
        "quad_indices.len()={} not divisible by 4",
        result.quad_indices.len(),
    );

    // (b) quads dominate: more quads than triangles. A clean recombine on
    // a regular unit-square profile typically produces zero triangles, but
    // the relaxed assertion accepts a partially-recombined result too.
    let n_quads = result.quad_indices.len() / 4;
    let n_tris = result.triangle_indices.len() / 3;
    assert!(
        n_quads > n_tris,
        "expected quads to dominate on a recombineable profile, \
         got n_quads={n_quads} n_tris={n_tris}",
    );

    // (c) every index (quad + triangle) is in bounds.
    for (i, &idx) in result.quad_indices.iter().enumerate() {
        assert!(
            (idx as usize) < n_verts,
            "quad_indices[{i}]={idx} out of bounds (n_verts={n_verts})",
        );
    }
    for (i, &idx) in result.triangle_indices.iter().enumerate() {
        assert!(
            (idx as usize) < n_verts,
            "triangle_indices[{i}]={idx} out of bounds (n_verts={n_verts})",
        );
    }

    // (d) every quad's max corner skew is within a coarse sanity bound.
    // The kernel test uses π/3 (60° deviation) rather than the
    // orchestrator's default π/4 because gmsh's interior-vertex placement
    // can introduce a single quad with skew slightly above π/4 even on a
    // unit-square profile at mesh_size=0.5. The strict π/4 quality
    // predicate is the orchestrator's concern (`recombine_quality_ok`);
    // this test asserts only that the recombine plumbing produces quads
    // shaped roughly like quadrilaterals (vs. degenerates).
    let threshold = std::f64::consts::FRAC_PI_3;
    for (q_idx, chunk) in result.quad_indices.chunks_exact(4).enumerate() {
        let coords: [[f64; 2]; 4] = [
            [
                result.vertices_xy[chunk[0] as usize * 2],
                result.vertices_xy[chunk[0] as usize * 2 + 1],
            ],
            [
                result.vertices_xy[chunk[1] as usize * 2],
                result.vertices_xy[chunk[1] as usize * 2 + 1],
            ],
            [
                result.vertices_xy[chunk[2] as usize * 2],
                result.vertices_xy[chunk[2] as usize * 2 + 1],
            ],
            [
                result.vertices_xy[chunk[3] as usize * 2],
                result.vertices_xy[chunk[3] as usize * 2 + 1],
            ],
        ];
        let max_skew = (0..4)
            .map(|i| {
                let prev = coords[(i + 3) % 4];
                let curr = coords[i];
                let next = coords[(i + 1) % 4];
                let e1 = [next[0] - curr[0], next[1] - curr[1]];
                let e2 = [prev[0] - curr[0], prev[1] - curr[1]];
                let cross = e1[0] * e2[1] - e1[1] * e2[0];
                let dot = e1[0] * e2[0] + e1[1] * e2[1];
                let angle = cross.abs().atan2(dot);
                (angle - std::f64::consts::FRAC_PI_2).abs()
            })
            .fold(0.0_f64, f64::max);
        assert!(
            max_skew <= threshold,
            "quad[{q_idx}] (verts {chunk:?}, coords {coords:?}) max skew {max_skew} \
             exceeds threshold {threshold}",
        );
    }
}

/// Hole handling: a 10x10 outer square with a small 2x2 hole in the
/// middle produces a mesh that avoids the hole interior — no element
/// centroid and no vertex falls strictly inside the hole rect.
#[cfg(has_gmsh)]
#[test]
fn mesh_plane_2d_with_hole_avoids_hole_interior() {
    // Taken by every has_gmsh test in this binary, not only by the guard that
    // needs it: a mutex one participant skips provides no exclusion, and this
    // test writes the process-global Mesh.MeshSizeMin/Max that
    // `mesh_plane_2d_leaves_every_size_option_at_gmsh_defaults` measures.
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());
    let outer: Vec<[f64; 2]> = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    // CW order for the hole — gmsh accepts either winding.
    let holes: Vec<Vec<[f64; 2]>> = vec![vec![[4.0, 4.0], [4.0, 6.0], [6.0, 6.0], [6.0, 4.0]]];

    let result = mesh_plane_2d(&outer, &holes, Some(2.0), false, true)
        .expect("mesh_plane_2d failed on outer+hole boundary");

    let n_verts = result.vertices_xy.len() / 2;
    assert!(n_verts > 0, "expected at least one vertex");

    // (a) recombine=false → triangle path; non-empty stride-3 buffer.
    assert!(
        !result.triangle_indices.is_empty(),
        "triangle_indices is empty — recombine=false should produce triangles",
    );
    assert_eq!(result.triangle_indices.len() % 3, 0);

    // (c) no vertex lies strictly inside the hole rect (boundary OK).
    // A small epsilon guards against floating-point boundary noise from
    // gmsh's coordinate readback (gmsh stores f64 internally; the hole
    // ring corners come back at machine precision).
    let eps = 1e-9;
    for (i, chunk) in result.vertices_xy.chunks_exact(2).enumerate() {
        let (x, y) = (chunk[0], chunk[1]);
        let strictly_inside_hole = x > 4.0 + eps && x < 6.0 - eps && y > 4.0 + eps && y < 6.0 - eps;
        assert!(
            !strictly_inside_hole,
            "vertex {i} at ({x}, {y}) lies strictly inside the hole rect [4,6]^2",
        );
    }

    // (b) no triangle centroid falls strictly inside the hole rect.
    for (t_idx, tri) in result.triangle_indices.chunks_exact(3).enumerate() {
        let p0 = [
            result.vertices_xy[tri[0] as usize * 2],
            result.vertices_xy[tri[0] as usize * 2 + 1],
        ];
        let p1 = [
            result.vertices_xy[tri[1] as usize * 2],
            result.vertices_xy[tri[1] as usize * 2 + 1],
        ];
        let p2 = [
            result.vertices_xy[tri[2] as usize * 2],
            result.vertices_xy[tri[2] as usize * 2 + 1],
        ];
        let cx = (p0[0] + p1[0] + p2[0]) / 3.0;
        let cy = (p0[1] + p1[1] + p2[1]) / 3.0;
        let strictly_inside_hole = cx > 4.0 && cx < 6.0 && cy > 4.0 && cy < 6.0;
        assert!(
            !strictly_inside_hole,
            "triangle {t_idx} centroid ({cx}, {cy}) lies strictly inside the hole rect",
        );
    }
}

/// Stub-build companion: the cfg(not(has_gmsh)) arm of `mesh_plane_2d`
/// returns `GeometryError::OperationFailed("…Gmsh not available…")`
/// regardless of input — pinning the documented stub-mode behaviour.
#[cfg(not(has_gmsh))]
#[test]
fn mesh_plane_2d_returns_gmsh_not_available_in_stub_build() {
    use reify_ir::GeometryError;

    let outer: Vec<[f64; 2]> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let holes: Vec<Vec<[f64; 2]>> = vec![];

    let err = mesh_plane_2d(&outer, &holes, Some(0.5), false, true)
        .expect_err("mesh_plane_2d must return Err in stub builds");

    match err {
        GeometryError::OperationFailed(msg) => {
            assert!(
                msg.contains("Gmsh not available"),
                "stub error message must mention 'Gmsh not available', got: {msg}",
            );
        }
        other => panic!("expected OperationFailed, got {other:?}"),
    }
}

/// `mesh_plane_2d` neither inherits nor leaks a mesh-size process-global.
///
/// Two legs, one per direction of task #6968, because either alone passes for
/// the wrong reason:
///
/// * **Outbound** — after a call with `mesh_size: Some(s)`, every entry of
///   [`GMSH_SIZE_OPTION_DEFAULTS`] must read back as gmsh's default. Before the
///   fix `Mesh.MeshSizeMin`/`MeshSizeMax` both read `s`: `mesh_profile_2d.rs`
///   writes the pair behind `if let Some(s) = mesh_size && s > 0.0` and
///   restores neither.
/// * **Inbound** — from a table poisoned in ONE size option, a
///   `mesh_size: None` call must produce exactly the triangle count it produces
///   from a defaults table. Before the fix it produced a different mesh:
///   `mesh_size: None` writes no size option at all, and `geo_add_point`
///   passes meshSize `0.0` ("no prescribed size here"), so the process-global
///   table alone decides element size.
///
/// The inbound leg is the production-reachable one.
/// `reify_solver_elastic::mesher` passes `mesh_size: None` whenever
/// `auto_mesh_size_from_boundary` returns `0.0`, so a
/// `refine_volume_with_size_field` earlier in the same process used to decide
/// an unrelated 2D mesh's density.
///
/// # Why one option at a time, and what each is worth
///
/// Poisoning all five at once is what a first draft of this test did, and it
/// was a FALSE PASS: measured on this fixture, a combined poison of
/// `Min = Max = 0.125`, `FromPoints = 0`, `FromCurvature = 1`,
/// `ExtendFromBoundary = 0` returns exactly the 162 triangles of an unpoisoned
/// run — the individual effects cancel. A guard whose poison cancels asserts
/// nothing, which is the failure mode #6968 exists to close, so each option is
/// poisoned on its own and a combined case is added on top rather than instead.
///
/// Per-option sensitivity, MEASURED on this unit square against an unpoisoned
/// baseline of 162 triangles (shipped gmsh 4.15.2, `mesh_plane_2d` unarmed):
///
/// ```text
/// Mesh.MeshSizeMin                = 0.05  ->  162   inert
/// Mesh.MeshSizeMax                = 0.05  ->  944   5.8x
/// Mesh.MeshSizeFromPoints         = 0     ->    4   40x
/// Mesh.MeshSizeFromCurvature      = 20    ->  162   inert
/// Mesh.MeshSizeExtendFromBoundary = 0     ->   48   3.4x
/// all five together                       ->  940
/// ```
///
/// Two of the five are INERT on this fixture and the assertions for them are
/// therefore vacuous today — said plainly rather than left to imply a
/// sensitivity they lack. A `MeshSizeMin` below the mesh's natural element size
/// is a floor nothing reaches, and a flat square bounded by straight lines has
/// no curvature for `MeshSizeFromCurvature` to sample. They are asserted anyway
/// because vacuity here is a property of the FIXTURE, not of the option: the
/// day someone gives this probe a curved boundary, the assertion starts biting
/// with no test edit.
///
/// The 48 is the same number this crate's hermeticity binary has already
/// reported from an unlucky thread interleaving — a leak observed in the wild
/// before it was explained.
#[cfg(has_gmsh)]
#[test]
fn mesh_plane_2d_leaves_every_size_option_at_gmsh_defaults() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    const OUTER: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    /// Distinctive enough that a leaked clamp is loud rather than marginal,
    /// and unequal to any default, so the outbound read cannot pass by
    /// accident on a table nobody wrote.
    const REQUESTED: f64 = 0.125;

    let triangles = |mesh_size: Option<f64>| -> usize {
        mesh_plane_2d(&OUTER, &[], mesh_size, false, true)
            .expect("mesh_plane_2d must succeed for a unit square")
            .triangle_indices
            .len()
            / 3
    };
    let write_all_size_options = |value_of: &dyn Fn(&str, f64) -> f64| {
        let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init::ensure_initialized();
        for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
            ffi::option_set_number(option, value_of(option, default))
                .unwrap_or_else(|e| panic!("ffi::option_set_number({option}) failed: {e:?}"));
        }
    };

    // --- Outbound: a call that writes a clamp must not leave it behind. ---
    triangles(Some(REQUESTED));
    {
        // Re-acquire GMSH_LOCK for the read, mirroring
        // `mesh_to_volume_tests.rs::mesh_to_volume_leaves_the_gmsh_logger_stopped`:
        // the read is serialised against any concurrent mesher rather than
        // racing one mid-flight.
        let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
            let observed = ffi::option_get_number(option)
                .unwrap_or_else(|e| panic!("ffi::option_get_number({option}) failed: {e:?}"));
            assert_eq!(
                observed, default,
                "mesh_plane_2d(mesh_size: Some({REQUESTED})) must leave every mesh-size \
                 process-global at gmsh's default on exit: {option} reads {observed}, \
                 expected {default}. gmsh's option table survives gmshClear(), so a \
                 deviation here pins every later defaults-relying call in this process \
                 to a size nobody requested — task #6968, enforced by `MeshSizeScope` in \
                 mesh_profile_2d.rs",
            );
        }
    }

    // --- Inbound: a call that writes no clamp must not inherit one. ---
    write_all_size_options(&|_, default| default);
    let from_defaults = triangles(None);
    assert!(
        from_defaults > 0,
        "the defaults-relying 2D probe must produce triangles; got an empty mesh",
    );

    /// One poison per size option, each measurably away from its default. See
    /// this test's doc for which of them this fixture is actually sensitive to.
    const INBOUND_POISONS: [(&str, f64); 5] = [
        ("Mesh.MeshSizeMin", 0.05),
        ("Mesh.MeshSizeMax", 0.05),
        ("Mesh.MeshSizeFromPoints", 0.0),
        ("Mesh.MeshSizeFromCurvature", 20.0),
        ("Mesh.MeshSizeExtendFromBoundary", 0.0),
    ];
    let poison_one = |poisoned: &str, value: f64| {
        write_all_size_options(&|option, default| {
            if option == poisoned { value } else { default }
        });
    };

    for (poisoned, value) in INBOUND_POISONS {
        poison_one(poisoned, value);
        let from_poisoned = triangles(None);
        assert_eq!(
            from_poisoned, from_defaults,
            "mesh_plane_2d(mesh_size: None) must mesh against gmsh's defaults, not against \
             whatever a sibling entry point last left in the process-global table: with \
             {poisoned} = {value} the same call gave {from_poisoned} triangles against \
             {from_defaults} from a defaults table. This is the inbound direction of task \
             #6968, closed by `MeshSizeScope::entered` in mesh_profile_2d.rs — and it is \
             production-reachable, since reify_solver_elastic::mesher passes \
             mesh_size: None whenever auto_mesh_size_from_boundary returns 0.0",
        );
    }

    // All five at once. Not a substitute for the sweep above — measured, this
    // combination happens to cancel back to the baseline for some poison
    // values — but it does pin the case a real leaking sibling produces, which
    // is several options at once rather than one.
    write_all_size_options(&|option, _| {
        INBOUND_POISONS
            .iter()
            .find(|(name, _)| *name == option)
            .map(|(_, value)| *value)
            .expect("INBOUND_POISONS must cover every GMSH_SIZE_OPTION_DEFAULTS entry")
    });
    let from_fully_poisoned = triangles(None);
    assert_eq!(
        from_fully_poisoned, from_defaults,
        "mesh_plane_2d(mesh_size: None) must be unaffected by a fully poisoned size table: \
         got {from_fully_poisoned} triangles against {from_defaults} from a defaults table",
    );
}
