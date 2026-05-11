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
//! - **Classifier units** (Loft / SweepGuided / twisted-Sweep `None` cases):
//!   `crates/reify-eval/src/sweep_classifier.rs#tests`
//! - **Classifier e2e** (wired through `Engine::build`):
//!   `crates/reify-eval/tests/swept_kind_classifier_e2e.rs`
//! - **`dispatch_volume_mesh` 8-case truth table** (`force_tet` /
//!   `require_hex_wedge` diagnostics surface):
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
