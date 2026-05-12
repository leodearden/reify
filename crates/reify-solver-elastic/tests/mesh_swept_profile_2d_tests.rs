//! Integration tests for [`reify_solver_elastic::mesher::mesh_swept_profile_2d`].
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! These tests round-trip a polygonal `ProfileBoundary` through the
//! orchestrator into a typed `Mesh2dReport`. They gate the libgmsh-
//! dependent assertions at runtime on `reify_kernel_gmsh::GMSH_AVAILABLE`
//! — the orchestrator itself isn't `cfg(has_gmsh)`-aware (and we don't
//! plumb a build.rs cfg through the crate per the task plan), so a stub
//! build simply asserts the `GmshUnavailable` error path.

use reify_kernel_gmsh::GMSH_AVAILABLE;
use reify_solver_elastic::mesher::{
    Mesh2d, Mesh2dError, Mesh2dOptions, ProfileBoundary, SweepElementTarget, mesh_swept_profile_2d,
    recombine_quality_ok,
};

fn unit_square_boundary() -> ProfileBoundary {
    ProfileBoundary {
        outer: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        holes: vec![],
    }
}

/// `SweepElementTarget::WedgeOnly` on a unit square produces a
/// triangle mesh with `recombine_attempted=false` and vacuously
/// `recombine_quality_ok=true`. All vertices land within the unit
/// square; the index buffer is non-empty stride-3.
#[test]
fn mesh_swept_profile_2d_wedge_target_unit_square_returns_triangles() {
    let boundary = unit_square_boundary();
    // Force determinism so the test result is reproducible across runs;
    // use an explicit mesh size so auto-size derivation doesn't kick in
    // (auto-size for a unit-square outer ring is 1.0, which on its own
    // can yield a single triangle pair — still valid, but the explicit
    // 0.5 keeps the test aligned with the kernel smoke test).
    let options = Mesh2dOptions {
        deterministic: true,
        mesh_size: Some(0.5),
        ..Default::default()
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::WedgeOnly, &options);

    if !GMSH_AVAILABLE {
        // Stub build: the underlying mesh_plane_2d returns
        // GeometryError::OperationFailed("Gmsh not available"), which the
        // orchestrator maps to Mesh2dError::GmshUnavailable.
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}",),
        }
        return;
    }

    let report = result.expect("WedgeOnly mesh_swept_profile_2d failed");

    // (b) recombine_attempted is false for WedgeOnly.
    assert!(
        !report.recombine_attempted,
        "WedgeOnly must NOT attempt recombination",
    );
    // (c) recombine_quality_ok is vacuously true (no quads to check).
    assert!(
        report.recombine_quality_ok,
        "WedgeOnly recombine_quality_ok must be true (vacuous — no quads)",
    );

    // (a, d, e, f) Triangle variant with valid stride-3 indices, even
    // vertex buffer length, and every vertex inside the unit square.
    match report.mesh {
        Mesh2d::Triangle { vertices, indices } => {
            assert!(
                !indices.is_empty(),
                "WedgeOnly triangle indices must be non-empty",
            );
            assert_eq!(
                indices.len() % 3,
                0,
                "WedgeOnly triangle indices must be stride-3",
            );
            assert_eq!(
                vertices.len() % 2,
                0,
                "vertices buffer (flat XY) must be even-length",
            );
            let n_verts = vertices.len() / 2;
            for &idx in &indices {
                assert!(
                    (idx as usize) < n_verts,
                    "triangle index {idx} out of bounds (n_verts={n_verts})",
                );
            }
            // Every vertex within the unit-square bounds (allowing a
            // tiny epsilon for gmsh's f64→f32 readback round-trip).
            for chunk in vertices.chunks_exact(2) {
                let (x, y) = (chunk[0], chunk[1]);
                assert!(
                    (-1e-5..=1.0 + 1e-5).contains(&x) && (-1e-5..=1.0 + 1e-5).contains(&y),
                    "vertex ({x}, {y}) outside unit square",
                );
            }
        }
        Mesh2d::Quad { .. } => panic!("WedgeOnly must return Triangle, not Quad"),
    }
}

/// `SweepElementTarget::HexPreferred` on a unit square recombines
/// cleanly: returns a `Mesh2d::Quad` with `recombine_attempted=true`,
/// `recombine_quality_ok=true`, stride-4 indices, and every quad
/// passes the orchestrator's default π/4 skew threshold.
#[test]
fn mesh_swept_profile_2d_hex_preferred_unit_square_recombines_cleanly() {
    let boundary = unit_square_boundary();
    // mesh_size > boundary edge keeps the recombined output to one or two
    // perfectly-square quads. With the auto-derived size of 1.0, gmsh
    // subdivides interior edges and produces a quad with skew slightly
    // above π/4 — that fall-back behaviour is exercised in the next pair.
    let options = Mesh2dOptions {
        mesh_size: Some(2.0),
        deterministic: true,
        ..Default::default()
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::HexPreferred, &options);

    if !GMSH_AVAILABLE {
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}",),
        }
        return;
    }

    let report = result.expect("HexPreferred mesh_swept_profile_2d failed on unit square");

    // (b) recombine_attempted is true for HexPreferred.
    assert!(
        report.recombine_attempted,
        "HexPreferred must record recombine_attempted=true",
    );
    // (c) recombine_quality_ok is true on a regular unit-square profile.
    assert!(
        report.recombine_quality_ok,
        "regular unit-square profile must pass the π/4 quality predicate \
         under HexPreferred",
    );

    // (a, d, e, f) Quad variant; stride-4 indices; in-bounds; every
    // vertex inside the unit square; recombine_quality_ok predicate
    // independently returns true at the π/4 threshold.
    match report.mesh {
        Mesh2d::Quad { vertices, indices } => {
            assert!(
                !indices.is_empty(),
                "HexPreferred quad indices must be non-empty"
            );
            assert_eq!(indices.len() % 4, 0, "quad indices must be stride-4",);
            assert_eq!(
                vertices.len() % 2,
                0,
                "vertices buffer (flat XY) must be even-length",
            );
            let n_verts = vertices.len() / 2;
            for &idx in &indices {
                assert!(
                    (idx as usize) < n_verts,
                    "quad index {idx} out of bounds (n_verts={n_verts})",
                );
            }
            for chunk in vertices.chunks_exact(2) {
                let (x, y) = (chunk[0], chunk[1]);
                assert!(
                    (-1e-5..=1.0 + 1e-5).contains(&x) && (-1e-5..=1.0 + 1e-5).contains(&y),
                    "vertex ({x}, {y}) outside unit square",
                );
            }
            assert!(
                recombine_quality_ok(&vertices, &indices, std::f64::consts::FRAC_PI_4),
                "recombine_quality_ok(π/4) must hold on the produced quad mesh",
            );
        }
        Mesh2d::Triangle { .. } => {
            panic!("HexPreferred on a regular unit-square profile must return Quad")
        }
    }
}

fn assert_triangle_fallback(report: reify_solver_elastic::mesher::Mesh2dReport, label: &str) {
    assert!(
        report.recombine_attempted,
        "{label}: HexPreferred must record recombine_attempted=true even after fall-back",
    );
    assert!(
        !report.recombine_quality_ok,
        "{label}: fall-back must record recombine_quality_ok=false",
    );
    match report.mesh {
        Mesh2d::Triangle { vertices, indices } => {
            assert!(
                !indices.is_empty(),
                "{label}: triangle indices must be non-empty"
            );
            assert_eq!(
                indices.len() % 3,
                0,
                "{label}: triangle indices must be stride-3",
            );
            let n_verts = vertices.len() / 2;
            for &idx in &indices {
                assert!(
                    (idx as usize) < n_verts,
                    "{label}: triangle index {idx} out of bounds (n_verts={n_verts})",
                );
            }
        }
        Mesh2d::Quad { .. } => {
            panic!("{label}: HexPreferred fall-back must return Triangle, not Quad")
        }
    }
}

/// HexPreferred fall-back: a pointy triangular profile cannot be
/// recombined into a clean quad mesh, so the orchestrator returns
/// triangles with `recombine_attempted=true` + `recombine_quality_ok=false`.
#[test]
fn mesh_swept_profile_2d_hex_preferred_pointy_triangle_falls_back_to_triangles() {
    let boundary = ProfileBoundary {
        outer: vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
        holes: vec![],
    };
    let options = Mesh2dOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::HexPreferred, &options);

    if !GMSH_AVAILABLE {
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}",),
        }
        return;
    }

    let report = result.expect("HexPreferred mesh_swept_profile_2d should fall back, not error");
    assert_triangle_fallback(report, "pointy triangle");
}

/// HexPreferred fall-back via threshold: a regular profile with an
/// impossibly tight skew threshold (0.01 rad) forces the quality
/// predicate to reject every quad gmsh recombination can produce,
/// triggering the triangle fall-back path even for a unit square.
#[test]
fn mesh_swept_profile_2d_hex_preferred_tight_threshold_falls_back_to_triangles() {
    let boundary = unit_square_boundary();
    let options = Mesh2dOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        recombine_skew_threshold: 0.01,
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::HexPreferred, &options);

    if !GMSH_AVAILABLE {
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}",),
        }
        return;
    }

    let report = result.expect("HexPreferred mesh_swept_profile_2d should fall back, not error");
    assert_triangle_fallback(report, "tight threshold");
}
