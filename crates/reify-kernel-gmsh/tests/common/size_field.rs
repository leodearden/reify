//! The uniform [`BackgroundSizeField`] that this crate's gmsh-driving test
//! binaries remesh under: `tests/mesh_to_volume_clamp_hermeticity.rs` and
//! `tests/mesher_poison_recovery.rs`.
//!
//! It is an instrument rather than a fixture under test — what varies between
//! the two consumers is what they measure while the field is held constant, so
//! a second copy would be free to drift into sizing something other than what
//! its consumer's assertions name. `tests/background_size_field_tests.rs`
//! builds its own cube deliberately: there the mesh IS the subject, and
//! sharing one would couple the serialiser's tests to their own input.
//!
//! Declared by `#[path]` from each binary, following `common/clamp_probe.rs`:
//! `common/mod.rs` states its own scope is the #6200 geometry fixtures, and
//! `reify_test_support` — where a shared fixture would otherwise belong —
//! cannot name [`BackgroundSizeField`] without taking a reify-kernel-gmsh
//! dependency edge that `common/mod.rs` records as structurally unwanted.

#![allow(dead_code)]

use reify_ir::{ElementOrderTag, VolumeConnectivity, VolumeMesh};
use reify_kernel_gmsh::BackgroundSizeField;

/// A uniform [`BackgroundSizeField`] of `size` spanning the unit cube.
///
/// The 6-tet Kuhn decomposition over the cube's 8 corners is the smallest mesh
/// that spans the box; a uniform field needs no more resolution than that.
pub fn uniform_unit_cube_size_field(size: f64) -> BackgroundSizeField {
    #[rustfmt::skip]
    let vm = VolumeMesh {
        vertices: vec![
            0.0_f32, 0.0, 0.0,
            1.0, 0.0, 0.0,
            1.0, 1.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
            1.0, 0.0, 1.0,
            1.0, 1.0, 1.0,
            0.0, 1.0, 1.0,
        ],
        connectivity: VolumeConnectivity::Tet {
            indices: vec![
                0, 1, 2, 6,
                0, 1, 5, 6,
                0, 3, 2, 6,
                0, 3, 7, 6,
                0, 4, 5, 6,
                0, 4, 7, 6,
            ],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    };
    BackgroundSizeField::from_tet_mesh(&vm, &vec![size; 8])
        .unwrap_or_else(|e| panic!("uniform size field must be constructible: {e:?}"))
}
