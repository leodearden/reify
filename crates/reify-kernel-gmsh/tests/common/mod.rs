//! Shared fixtures for the #6200 fill/coverage guards.
//!
//! # Why this file exists
//!
//! `tests/fill_metrics_tests.rs`, `tests/volume_fill_fraction.rs` and
//! `tests/classify_feature_angle.rs` are three views of the same defect
//! (arithmetic / symptom / mechanism) and therefore need the same geometry.
//! Written independently they carried three verbatim copies of
//! `prismatic_box_mesh` and two of `unwelded_prismatic_box_mesh` — ~150 lines
//! of duplication whose real cost is *drift*: a fixture corrected in one copy
//! and not the others silently makes the three guards disagree about what
//! "a box" is, which is exactly the class of gap that let #6200 survive.
//!
//! A `tests/common/` subdirectory (rather than a sibling `tests/*.rs` file) is
//! the cargo idiom for this: files under `tests/` are each compiled as their
//! own test binary, files under `tests/common/` are not.
//!
//! **Scoped deliberately to this crate, not `reify-test-support`.** The wider
//! hoist — one box fixture for the whole workspace, replacing this crate's
//! other copies in `mesh_to_volume_tests.rs` / `repair_tests.rs` and
//! `reify-kernel-manifold`'s `test_fixtures.rs` — is the right end state but
//! touches files outside #6200's scope. This module removes the duplication
//! that #6200 itself introduced.
//!
//! Every consumer is `#[allow(dead_code)]`-tolerant by construction: each test
//! binary compiles its own copy of this module and uses only part of it, so
//! the unused remainder must not be an error under `-D warnings`.

#![allow(dead_code)]

use reify_ir::Mesh;

// ---------------------------------------------------------------------------
// Tolerances
// ---------------------------------------------------------------------------

/// Relative tolerance for any assertion whose error budget is dominated by f32
/// coordinate storage.
///
/// Derived, not tuned. `Mesh::vertices` / `VolumeMesh::vertices` are `Vec<f32>`
/// — 24-bit mantissa, relative ulp <= 2^-23 ~ 1.19e-7. Both a tet volume and
/// the divergence-theorem surface integral are degree-3 forms in the
/// coordinates, so storage-induced relative error is bounded by
/// ~ 3 x 1.19e-7 ~ 3.6e-7. 1e-6 clears that ceiling ~3x while sitting five
/// orders of magnitude below the ~26% defect #6200 guards against, so it is
/// neither tight enough to flake nor loose enough to hide the bug.
///
/// Concretely: 0.1 is not representable in binary and stores as
/// 0.100000001490116…, so a 1.0x0.1x0.1 box measures 1.000000029802e-2 against
/// an exact 1e-2 — relative 2.98e-8, pure coordinate round-off rather than any
/// error in the summation. (#6154 independently measured the identical
/// 1.0000000298e-2 for the realized fixture's AABB.)
pub const F32_STORAGE_REL: f64 = 1e-6;

/// Assert `actual` matches `expected` to within `rel` RELATIVE error.
///
/// The tolerance is an explicit parameter rather than a file-level constant so
/// a call site that needs a looser band (curved geometry, say) states its own
/// number at the point of use instead of silently loosening every other
/// assertion in the file.
#[track_caller]
pub fn assert_rel(actual: f64, expected: f64, rel: f64, what: &str) {
    let err = (actual - expected).abs() / expected.abs().max(f64::MIN_POSITIVE);
    assert!(
        err <= rel,
        "{what}: got {actual:.12e}, expected {expected:.12e} (relative error {err:.3e} > {rel:.3e})"
    );
}

// ---------------------------------------------------------------------------
// Box fixtures
// ---------------------------------------------------------------------------

/// Axis-aligned box spanning `[0,lx] x [0,ly] x [0,lz]`: 8 vertices / 12
/// outward-wound triangles, enclosed volume exactly `lx * ly * lz`.
///
/// Originally an inline copy of
/// `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:13-48` (itself a
/// copy of `crates/reify-kernel-manifold/src/test_fixtures.rs:37-67`),
/// generalised to arbitrary extents.
///
/// The winding is vetted OUTWARD (unlike `through_thickness_tests.rs`'s
/// `slab_surface_mesh`), which is a precondition for every signed-volume and
/// divergence-theorem assertion built on it.
pub fn prismatic_box_mesh(lx: f32, ly: f32, lz: f32) -> Mesh {
    Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            lx, 0.0, 0.0, // 1
            lx, ly, 0.0, // 2
            0.0, ly, 0.0, // 3
            0.0, 0.0, lz, // 4
            lx, 0.0, lz, // 5
            lx, ly, lz, // 6
            0.0, ly, lz, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            // -Z bottom (outward = -Z, so CW from +Z view)
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

/// The unit cube — `prismatic_box_mesh(1.0, 1.0, 1.0)`, enclosed volume 1.0.
pub fn unit_cube_mesh() -> Mesh {
    prismatic_box_mesh(1.0, 1.0, 1.0)
}

/// Per-face UNWELDED box: 24 vertices (each of the 6 faces carrying its own 4
/// bit-identical corner copies) / 12 triangles, same outward winding as
/// [`prismatic_box_mesh`].
///
/// This is the shape `OcctKernel::tessellate` actually emits for a
/// planar-faced solid (`kernel_real.rs:452`), i.e. what the production
/// surface→volume path receives BEFORE `RepairConfig`'s weld pre-stage runs.
/// Enclosed volume must be identical to the welded form: the
/// divergence-theorem sum is a per-triangle integral against the origin and so
/// is welding-independent.
pub fn unwelded_prismatic_box_mesh(lx: f32, ly: f32, lz: f32) -> Mesh {
    // Corner coordinates, indexed exactly as in `prismatic_box_mesh`.
    let c = [
        [0.0, 0.0, 0.0],
        [lx, 0.0, 0.0],
        [lx, ly, 0.0],
        [0.0, ly, 0.0],
        [0.0, 0.0, lz],
        [lx, 0.0, lz],
        [lx, ly, lz],
        [0.0, ly, lz],
    ];
    // Per-face corner quads, in the same cyclic order the welded fixture uses.
    let faces: [[usize; 4]; 6] = [
        [0, 1, 2, 3], // -Z
        [4, 5, 6, 7], // +Z
        [0, 1, 5, 4], // -Y
        [3, 7, 6, 2], // +Y
        [0, 4, 7, 3], // -X
        [1, 2, 6, 5], // +X
    ];
    // Face-local triangle slots, matching the welded fixture's winding face
    // for face. Only the -Z face differs, because its welded form is written
    // (0,2,1) (0,3,2) rather than the (0,1,2) (0,2,3) the others use.
    let tri_slots: [[usize; 6]; 6] = [
        [0, 2, 1, 0, 3, 2], // -Z
        [0, 1, 2, 0, 2, 3], // +Z
        [0, 1, 2, 0, 2, 3], // -Y
        [0, 1, 2, 0, 2, 3], // +Y
        [0, 1, 2, 0, 2, 3], // -X
        [0, 1, 2, 0, 2, 3], // +X
    ];

    let mut vertices = Vec::with_capacity(24 * 3);
    let mut indices = Vec::with_capacity(36);
    for (f, quad) in faces.iter().enumerate() {
        let base = (f * 4) as u32;
        for &corner in quad {
            vertices.extend_from_slice(&c[corner]);
        }
        for &slot in &tri_slots[f] {
            indices.push(base + slot as u32);
        }
    }
    Mesh {
        vertices,
        indices,
        normals: None,
    }
}

// ---------------------------------------------------------------------------
// Curved fixture
// ---------------------------------------------------------------------------

/// Closed, outward-wound tessellated cylinder: an `n`-gon prism of radius `r`
/// and height `h`, axis along +Z, base on `z = 0`, centred on the Z axis.
///
/// `2n + 2` vertices (`n` per rim plus a centre vertex per cap) and `4n`
/// triangles (`2n` lateral, `n` per cap). Vertex 0 is placed at angle 0 and
/// the ring is walked CCW viewed from +Z, so for even `n` there are vertices
/// at 0°, 90°, 180° and 270° and the XY AABB is exactly `2r x 2r`.
///
/// # Why the guards need a curved body
///
/// `CLASSIFY_FEATURE_ANGLE` is a GLOBAL production parameter: it changes the
/// B-rep decomposition of every body `mesh_to_volume` meshes, not just boxes.
/// Every other fixture here is axis-aligned and prismatic, where each face is
/// exactly planar and every dihedral is exactly 90°. A cylinder exercises the
/// other half of the constant's blast radius:
///
/// - the two cap rims are genuine 90° feature edges (registered as sharp only
///   *after* #6200's fix), while
/// - the lateral surface is a chain of facets whose normals turn by `360/n`
///   degrees per step — smooth for large `n`, but *sharper than the 45°
///   threshold* once `n < 8`, which is when gmsh starts splitting the barrel
///   into per-facet patches that `create_geometry` must reparametrize
///   individually.
///
/// Closed-form volume of the tessellated body (NOT `pi r^2 h` — this is a
/// prism over an inscribed regular `n`-gon): `(n/2) * r^2 * sin(2*pi/n) * h`.
pub fn tessellated_cylinder_mesh(r: f32, h: f32, n: usize) -> Mesh {
    assert!(n >= 3, "a cylinder tessellation needs at least 3 segments");
    let mut vertices: Vec<f32> = Vec::with_capacity((2 * n + 2) * 3);
    // Bottom rim: 0..n. Top rim: n..2n.
    for z in [0.0f32, h] {
        for k in 0..n {
            let theta = 2.0 * std::f64::consts::PI * (k as f64) / (n as f64);
            vertices.push(r * theta.cos() as f32);
            vertices.push(r * theta.sin() as f32);
            vertices.push(z);
        }
    }
    // Cap centres: 2n (bottom), 2n+1 (top).
    vertices.extend_from_slice(&[0.0, 0.0, 0.0]);
    vertices.extend_from_slice(&[0.0, 0.0, h]);

    let bottom_centre = (2 * n) as u32;
    let top_centre = (2 * n + 1) as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(4 * n * 3);
    for k in 0..n {
        let k1 = ((k + 1) % n) as u32;
        let (b0, b1) = (k as u32, k1);
        let (t0, t1) = (b0 + n as u32, b1 + n as u32);
        // Lateral quad (b0, b1, t1, t0), outward normal pointing away from the
        // axis: with the rim walked CCW from +Z, (b0, b1, t1) is outward.
        indices.extend_from_slice(&[b0, b1, t1]);
        indices.extend_from_slice(&[b0, t1, t0]);
        // Bottom cap, outward normal -Z, so wound CW viewed from +Z.
        indices.extend_from_slice(&[bottom_centre, b1, b0]);
        // Top cap, outward normal +Z, so wound CCW viewed from +Z.
        indices.extend_from_slice(&[top_centre, t0, t1]);
    }
    Mesh {
        vertices,
        indices,
        normals: None,
    }
}

/// Exact enclosed volume of [`tessellated_cylinder_mesh`], in closed form.
///
/// `(n/2) * r^2 * sin(2*pi/n) * h` — the area of an inscribed regular `n`-gon
/// times the height. Kept next to the fixture so a call site never has to
/// re-derive it, and so the divergence-theorem sum is checkable against an
/// independent closed form rather than against itself.
pub fn tessellated_cylinder_volume(r: f64, h: f64, n: usize) -> f64 {
    0.5 * (n as f64) * r * r * (2.0 * std::f64::consts::PI / n as f64).sin() * h
}
