//! Integration tests: mesh-quality + structural contract on synthetic swept-body fixtures.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #13.
//!
//! # Scope
//!
//! This file exercises the **meshing-pipeline layer** only:
//! `mesh_swept_profile_2d` (task #6) → `sweep_2d_mesh_to_3d` (task #7).
//! It does NOT import the classifier (`classify_swept_body`, `SweptKind`) or
//! the dispatcher (`dispatch_volume_mesh`, `VolumeMeshOutcome`) — both live in
//! `reify-eval`, which already depends on `reify-solver-elastic`, so adding
//! `reify-eval` as a dev-dep would create a circular dependency.
//!
//! # Existing coverage this file relies on
//!
//! The following three reify-eval test points cover the task-description claims
//! that cannot be exercised from this crate (classifier + dispatcher live in
//! `reify-eval`, which would create a circular dev-dep if imported here):
//!
//! (a) **Classifier units** — Loft / SweepGuided / twisted-Sweep `None` cases;
//!   `classify_swept_body` catch-all arms; `SweptKind` discriminants:
//!   `crates/reify-eval/src/sweep_classifier.rs#tests`
//!
//! (b) **Classifier e2e** — `classify_swept_body` wired through `Engine::build`
//!   on realistic `GeomOp` sequences:
//!   `crates/reify-eval/tests/swept_kind_classifier_e2e.rs`
//!
//! (c) **`dispatch_volume_mesh` 8-case truth table** — canonical `force_tet` /
//!   `require_hex_wedge` diagnostic surface (the "diagnostics" claims in the
//!   task description); added by PRD task #2989:
//!   `crates/reify-eval/src/engine_build.rs#dispatch_volume_mesh_tests` (L4488+)
//!
//! # Gmsh gating
//!
//! Libgmsh-dependent assertions are gated on `reify_kernel_gmsh::GMSH_AVAILABLE`.
//! Stub builds assert `Err(Mesh2dError::GmshUnavailable)` and early-return,
//! following the pattern in `tests/mesh_swept_profile_2d_tests.rs`.

use std::f64::consts::PI;

use reify_kernel_gmsh::GMSH_AVAILABLE;
use reify_solver_elastic::{
    check_sweep_through_thickness, derive_layer_count, mesh_swept_profile_2d, sweep_2d_mesh_to_3d,
    Mesh2d, Mesh2dError, Mesh2dOptions, Mesh2dReport, ProfileBoundary, SweepElementTarget,
    SweepError, SweepParams, SweptConnectivity, SweptMesh3d, ThroughThicknessSweepWarning,
};

// ---------------------------------------------------------------------------
// Shared fixture helpers
// ---------------------------------------------------------------------------

/// Build a rectangular `ProfileBoundary` with a CCW outer ring and no holes.
///
/// Corners are `(0,0)` → `(w,0)` → `(w,h)` → `(0,h)` (counter-clockwise),
/// mirroring the `unit_square_boundary()` style in `mesh_swept_profile_2d_tests.rs`.
fn rect_boundary(w: f64, h: f64) -> ProfileBoundary {
    ProfileBoundary {
        outer: vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]],
        holes: vec![],
    }
}

/// Assert that a `SweptMesh3d` has the expected hex-element dimensions.
///
/// Checks:
/// - `connectivity` is `SweptConnectivity::Hex`.
/// - `indices.len() == 8 * expected_n_base_quads * expected_layers`.
/// - `vertices.len() == 3 * (expected_layers + 1) * expected_n_base_vertices`.
/// - `mesh.layers == expected_layers`.
fn assert_swept_hex_dimensions(
    mesh: &SweptMesh3d,
    expected_layers: usize,
    expected_n_base_quads: usize,
    expected_n_base_vertices: usize,
) {
    match &mesh.connectivity {
        SweptConnectivity::Hex { indices } => {
            assert_eq!(
                indices.len(),
                8 * expected_n_base_quads * expected_layers,
                "hex indices.len() must be 8 * n_base_quads * K \
                 (n_base_quads={expected_n_base_quads}, K={expected_layers})",
            );
        }
        SweptConnectivity::Wedge { .. } => {
            panic!("expected Hex connectivity, got Wedge");
        }
    }
    assert_eq!(
        mesh.vertices.len(),
        3 * (expected_layers + 1) * expected_n_base_vertices,
        "vertex buffer must be 3 * (K+1) * n_base_vertices \
         (K={expected_layers}, n_base_vertices={expected_n_base_vertices})",
    );
    assert_eq!(
        mesh.layers, expected_layers,
        "swept.layers must equal K={expected_layers}",
    );
}

/// Assert that a `SweptMesh3d` has the expected wedge-element dimensions.
///
/// Checks:
/// - `connectivity` is `SweptConnectivity::Wedge`.
/// - `indices.len() == 6 * expected_n_base_tris * expected_layers`.
/// - `vertices.len() == 3 * (expected_layers + 1) * expected_n_base_vertices`.
/// - `mesh.layers == expected_layers`.
fn assert_swept_wedge_dimensions(
    mesh: &SweptMesh3d,
    expected_layers: usize,
    expected_n_base_tris: usize,
    expected_n_base_vertices: usize,
) {
    match &mesh.connectivity {
        SweptConnectivity::Wedge { indices } => {
            assert_eq!(
                indices.len(),
                6 * expected_n_base_tris * expected_layers,
                "wedge indices.len() must be 6 * n_base_tris * K \
                 (n_base_tris={expected_n_base_tris}, K={expected_layers})",
            );
        }
        SweptConnectivity::Hex { .. } => {
            panic!("expected Wedge connectivity, got Hex");
        }
    }
    assert_eq!(
        mesh.vertices.len(),
        3 * (expected_layers + 1) * expected_n_base_vertices,
        "vertex buffer must be 3 * (K+1) * n_base_vertices \
         (K={expected_layers}, n_base_vertices={expected_n_base_vertices})",
    );
    assert_eq!(
        mesh.layers, expected_layers,
        "swept.layers must equal K={expected_layers}",
    );
}

/// Assert that `check_sweep_through_thickness(layers, min_layers)` returns `None`.
///
/// Panics with a human-readable message that names `mesh_size` and
/// `sweep_subdivisions` (the two knobs callers can adjust) if the check fails.
fn through_thickness_must_pass(layers: usize, min_layers: usize) {
    assert!(
        check_sweep_through_thickness(layers, min_layers).is_none(),
        "through-thickness check failed: {layers} layers < {min_layers} minimum. \
         Decrease mesh_size or set an explicit sweep_subdivisions.",
    );
}

// ---------------------------------------------------------------------------
// Fixture tests
// ---------------------------------------------------------------------------

/// Extruded plate (100×100×2 mm, hex-eligible).
///
/// Profiles a 100×100 square with `HexPreferred`; sweeps 2 mm along Z.
/// Asserts:
/// - On Gmsh builds: quad 2D mesh → hex 3D connectivity with
///   `indices.len() == 8 * n_base_quads * K` and vertex buffer
///   `len == 3 * (K+1) * n_base_vertices`.
/// - Through-thickness: `check_sweep_through_thickness(K, 2).is_none()`.
/// - On stub builds: `Err(GmshUnavailable)` early-return.
#[test]
fn extruded_plate_hex_mesh_succeeds_with_expected_element_count() {
    let boundary = rect_boundary(100.0, 100.0);
    let options = Mesh2dOptions {
        mesh_size: Some(20.0),
        deterministic: true,
        ..Mesh2dOptions::default()
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::HexPreferred, &options);

    if !GMSH_AVAILABLE {
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}"),
        }
        return;
    }

    let report = result.expect("extruded plate: mesh_swept_profile_2d failed");

    let (n_base_quads, n_base_vertices) = match &report.mesh {
        Mesh2d::Quad { vertices, indices } => {
            assert_eq!(indices.len() % 4, 0, "quad indices must be stride-4");
            assert!(!indices.is_empty(), "quad indices must be non-empty");
            (indices.len() / 4, vertices.len() / 2)
        }
        Mesh2d::Triangle { .. } => {
            panic!("extruded plate with HexPreferred should return Quad mesh")
        }
    };
    assert!(n_base_quads >= 1, "expected at least one base quad");

    let k = derive_layer_count(2.0, 1.0, 2);
    assert!(k >= 2, "derive_layer_count(2.0, 1.0, 2) must be >= 2, got {k}");

    let swept = sweep_2d_mesh_to_3d(
        &report.mesh,
        &SweepParams::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: 2.0,
        },
        k,
    )
    .expect("extruded plate: sweep_2d_mesh_to_3d failed");

    assert_swept_hex_dimensions(&swept, k, n_base_quads, n_base_vertices);
    through_thickness_must_pass(k, 2);
}

/// Revolved disc (R=50, H=2 mm, hex/wedge-eligible).
///
/// Uses the meridian rectangle `[0..50] × [0..2]` in the (x,y) plane and
/// revolves it π/2 around the Y axis.  A partial-arc keeps the fixture cheap;
/// arc correctness is already pinned by the `revolve_*` unit tests in `sweep.rs`.
///
/// Accepts either `Mesh2d::Quad` (→ Hex) or `Mesh2d::Triangle` (→ Wedge): the
/// disc meridian can be aspect-ratio-difficult for the recombiner, so both
/// outcomes are valid.  The test asserts element-count consistency for whichever
/// variant is produced, plus the through-thickness contract.
///
/// On stub builds: `Err(GmshUnavailable)` early-return.
#[test]
fn revolved_disc_hex_or_wedge_mesh_succeeds() {
    // Meridian rectangle: 50 mm wide (radial), 2 mm tall (axial).
    let boundary = ProfileBoundary {
        outer: vec![[0.0, 0.0], [50.0, 0.0], [50.0, 2.0], [0.0, 2.0]],
        holes: vec![],
    };
    let options = Mesh2dOptions {
        mesh_size: Some(2.0),
        deterministic: true,
        ..Mesh2dOptions::default()
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::HexPreferred, &options);

    if !GMSH_AVAILABLE {
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}"),
        }
        return;
    }

    let report = result.expect("revolved disc: mesh_swept_profile_2d failed");

    let k: usize = 2;
    let params = SweepParams::Revolve {
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 1.0, 0.0],
        angle: PI / 2.0,
    };

    let swept =
        sweep_2d_mesh_to_3d(&report.mesh, &params, k).expect("revolved disc: sweep failed");

    match &report.mesh {
        Mesh2d::Quad { vertices, indices } => {
            let n_base_quads = indices.len() / 4;
            let n_base_verts = vertices.len() / 2;
            assert_swept_hex_dimensions(&swept, k, n_base_quads, n_base_verts);
        }
        Mesh2d::Triangle { vertices, indices } => {
            let n_base_tris = indices.len() / 3;
            let n_base_verts = vertices.len() / 2;
            assert_swept_wedge_dimensions(&swept, k, n_base_tris, n_base_verts);
        }
    }
    through_thickness_must_pass(k, 2);
}

/// Phase A contract: `SweepLinear` is byte-identical to `Extrude` on a meshed profile.
///
/// Meshes a 10×5 rectangle at `mesh_size=2.0` once, then sweeps it twice:
/// - `SweepParams::Extrude { axis: [0,0,1], length: 4.0 }` → `extrude_mesh`
/// - `SweepParams::SweepLinear { axis: [0,0,1], length: 4.0 }` → `linear_mesh`
///
/// Asserts that `extrude_mesh.vertices == linear_mesh.vertices` (byte-equal
/// `Vec<f32>`), `extrude_mesh.layers == linear_mesh.layers`, and the inner
/// `SweptConnectivity::Hex { indices }` buffers are byte-equal.
///
/// Pins the Phase A contract at integration mesh-density — the existing unit
/// test `sweep_linear_equals_extrude_same_axis_length` in `sweep.rs` only
/// covers a hand-rolled 3-vertex triangle; a regression that special-cases
/// `SweepLinear` in any allocation or transform path would surface here.
///
/// On stub builds: `Err(GmshUnavailable)` early-return.
#[test]
fn simple_linear_sweep_byte_identical_to_extrude_on_meshed_profile() {
    let boundary = rect_boundary(10.0, 5.0);
    let options = Mesh2dOptions {
        mesh_size: Some(2.0),
        deterministic: true,
        ..Mesh2dOptions::default()
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::HexPreferred, &options);

    if !GMSH_AVAILABLE {
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}"),
        }
        return;
    }

    let report = result.expect("linear sweep test: mesh_swept_profile_2d failed");

    let k = derive_layer_count(4.0, 2.0, 2);
    assert!(k >= 2, "derive_layer_count(4.0, 2.0, 2) must be >= 2, got {k}");

    let extrude_mesh = sweep_2d_mesh_to_3d(
        &report.mesh,
        &SweepParams::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: 4.0,
        },
        k,
    )
    .expect("linear sweep test: extrude sweep_2d_mesh_to_3d failed");

    let linear_mesh = sweep_2d_mesh_to_3d(
        &report.mesh,
        &SweepParams::SweepLinear {
            axis: [0.0, 0.0, 1.0],
            length: 4.0,
        },
        k,
    )
    .expect("linear sweep test: SweepLinear sweep_2d_mesh_to_3d failed");

    assert_eq!(
        extrude_mesh.vertices, linear_mesh.vertices,
        "Phase A: SweepLinear vertex buffer must be byte-identical to Extrude",
    );
    assert_eq!(
        extrude_mesh.layers, linear_mesh.layers,
        "Phase A: SweepLinear layers must equal Extrude layers",
    );
    match (&extrude_mesh.connectivity, &linear_mesh.connectivity) {
        (SweptConnectivity::Hex { indices: ei }, SweptConnectivity::Hex { indices: li }) => {
            assert_eq!(
                ei, li,
                "Phase A: SweepLinear hex index buffer must be byte-identical to Extrude",
            );
        }
        (SweptConnectivity::Wedge { indices: ei }, SweptConnectivity::Wedge { indices: li }) => {
            assert_eq!(
                ei, li,
                "Phase A: SweepLinear wedge index buffer must be byte-identical to Extrude",
            );
        }
        _ => panic!(
            "Phase A: Extrude and SweepLinear must produce the same connectivity discriminant",
        ),
    }
}

/// Drilled plate — Phase B positive case at the meshing-pipeline layer.
///
/// `ProfileBoundary` with a 20×20 rectangular hole centred in a 100×100 square.
/// The outer ring is CCW; the hole ring is CW (standard convention for holes).
///
/// This exercises Phase B's positive case: `mesh_swept_profile_2d` already
/// supports multiply-connected regions (`holes: Vec<Vec<[f64; 2]>>`), and the
/// sweep step is hole-agnostic (operates on the 2D index buffer regardless of
/// topology), so the same `n_base × K` formula holds.
///
/// **Scope note:** The Phase A classifier-rejection case (post-sweep modify ops
/// applied to an otherwise-swept body) is explicitly deferred to:
/// `crates/reify-eval/src/sweep_classifier.rs#tests`
///
/// On stub builds: `Err(GmshUnavailable)` early-return.
#[test]
fn drilled_plate_phase_b_positive_case_succeeds() {
    // Outer ring: CCW 100×100 square.
    // Hole ring: CW 20×20 square centred at (50,50) — i.e. corners at
    // (40,40)→(40,60)→(60,60)→(60,40), wound clockwise.
    let boundary = ProfileBoundary {
        outer: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
        holes: vec![vec![
            [40.0, 40.0],
            [40.0, 60.0],
            [60.0, 60.0],
            [60.0, 40.0],
        ]],
    };
    let options = Mesh2dOptions {
        mesh_size: Some(20.0),
        deterministic: true,
        ..Mesh2dOptions::default()
    };

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::HexPreferred, &options);

    if !GMSH_AVAILABLE {
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!("stub build: expected Err(GmshUnavailable), got {other:?}"),
        }
        return;
    }

    let report = result.expect("drilled plate: mesh_swept_profile_2d failed");

    // HexPreferred always attempts recombination.
    assert!(
        report.recombine_attempted,
        "drilled plate: HexPreferred must record recombine_attempted=true",
    );

    let k = derive_layer_count(2.0, 1.0, 2);
    assert!(k >= 2, "derive_layer_count(2.0, 1.0, 2) must be >= 2, got {k}");

    let swept = sweep_2d_mesh_to_3d(
        &report.mesh,
        &SweepParams::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: 2.0,
        },
        k,
    )
    .expect("drilled plate: sweep_2d_mesh_to_3d failed");

    match &report.mesh {
        Mesh2d::Quad { vertices, indices } => {
            let n_base_quads = indices.len() / 4;
            let n_base_verts = vertices.len() / 2;
            assert_swept_hex_dimensions(&swept, k, n_base_quads, n_base_verts);
        }
        Mesh2d::Triangle { vertices, indices } => {
            let n_base_tris = indices.len() / 3;
            let n_base_verts = vertices.len() / 2;
            assert_swept_wedge_dimensions(&swept, k, n_base_tris, n_base_verts);
        }
    }
    through_thickness_must_pass(k, 2);
}

/// Surface-pin test: verifies that every type, function, and constant used by
/// the fixture tests below can be resolved at compile time.  A regression that
/// renames or removes any of these re-exports breaks here *before* any fixture
/// test runs, giving an immediate, targeted compilation error.
#[test]
fn compiles_against_public_surface() {
    // Function-pointer casts verify exact signatures match the re-exports.
    let _: fn(&ProfileBoundary, SweepElementTarget, &Mesh2dOptions)
        -> Result<Mesh2dReport, Mesh2dError> = mesh_swept_profile_2d;
    let _: fn(&Mesh2d, &SweepParams, usize) -> Result<SweptMesh3d, SweepError> =
        sweep_2d_mesh_to_3d;
    let _: fn(f64, f64, usize) -> usize = derive_layer_count;
    let _: fn(usize, usize) -> Option<ThroughThicknessSweepWarning> =
        check_sweep_through_thickness;

    // GMSH_AVAILABLE: const bool from the kernel crate.
    let _: bool = GMSH_AVAILABLE;

    // Discriminant coverage — all variants must be reachable by name.
    let _t1 = SweepElementTarget::HexPreferred;
    let _t2 = SweepElementTarget::WedgeOnly;

    let _p1 = SweepParams::Extrude {
        axis: [0.0, 0.0, 1.0],
        length: 1.0,
    };
    let _p2 = SweepParams::Revolve {
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 1.0, 0.0],
        angle: PI / 2.0,
    };
    let _p3 = SweepParams::SweepLinear {
        axis: [0.0, 0.0, 1.0],
        length: 1.0,
    };

    let _e1 = SweepError::DegenerateAxis;
    let _e2 = SweepError::DegenerateMagnitude;
    let _e3 = SweepError::EmptyMesh2d;
    let _e4 = SweepError::InvalidLayerCount;

    let _c1 = SweptConnectivity::Wedge { indices: vec![] };
    let _c2 = SweptConnectivity::Hex { indices: vec![] };

    let warn = ThroughThicknessSweepWarning {
        layer_count: 1,
        min_layers: 2,
        message: "test".to_string(),
    };
    let _ = (warn.layer_count, warn.min_layers, warn.message);
}
