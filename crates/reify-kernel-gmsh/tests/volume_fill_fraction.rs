//! Acceptance guard: `mesh_to_volume` must produce a COMPLETE
//! tetrahedralization of a prismatic body (#6200).
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.
//!
//! # What this pins
//!
//! A tet mesh can be structurally perfect — every element valid, every index
//! in range — and still cover only part of the solid. Before #6200,
//! `classify_surfaces` was handed a 90° feature angle; a box's dihedral angle
//! is *exactly* 90° and gmsh's sharp-edge test is strictly-greater-than, so the
//! box's own edges were never registered as sharp, the B-rep decomposition
//! bounded a region smaller than the box, and HXT filled that smaller region.
//! Measured fill fractions before the fix: 0.862916 (unit cube), 0.736248
//! (1.0x0.1x0.1 m box), 0.735705 (0.2x0.1x0.1 m box).
//!
//! The companion guard `tests/classify_feature_angle.rs` pins the same defect
//! at its ROOT CAUSE (the B-rep entity census) rather than at this symptom.

#![cfg(has_gmsh)]

use reify_ir::{ElementOrderTag, Mesh};
use reify_kernel_gmsh::fill_metrics::{TetFillReport, enclosed_volume_of_surface, tet_fill_report};
use reify_kernel_gmsh::repair::RepairConfig;
use reify_kernel_gmsh::{GmshKernel, MeshingOptions, mesh_surface_to_volume_with_diagnostics};

/// Relative tolerance for the prismatic fill assertions.
///
/// Derived, not tuned. `VolumeMesh::vertices` is `Vec<f32>` — 24-bit mantissa,
/// relative ulp <= 2^-23 ~ 1.19e-7. A tet volume is a degree-3 form in the
/// coordinates, so storage-induced relative error is bounded by ~ 3 x 1.19e-7
/// ~ 3.6e-7. 1e-6 clears that ceiling ~3x while sitting five orders of
/// magnitude below the ~26% defect this file guards against, so it is neither
/// tight enough to flake nor loose enough to hide the bug.
const REL: f64 = 1e-6;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Inline copy of `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:13-48`
/// (itself a copy of `crates/reify-kernel-manifold/src/test_fixtures.rs:37-67`),
/// generalised to arbitrary extents.
///
/// 8 vertices / 12 outward-wound triangles; enclosed volume exactly
/// `lx * ly * lz`. Duplicated rather than dev-dep'ing on
/// `reify-kernel-manifold` to avoid an awkward layering — the crate already
/// carries three such cross-referenced copies. The winding is vetted outward
/// (unlike `through_thickness_tests.rs`'s `slab_surface_mesh`), which is a
/// precondition for every signed-volume assertion below.
fn prismatic_box_mesh(lx: f32, ly: f32, lz: f32) -> Mesh {
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

/// Per-face UNWELDED box: 24 vertices (each face carrying its own 4
/// bit-identical corner copies) / 12 triangles, same outward winding as
/// [`prismatic_box_mesh`].
///
/// This is the shape `OcctKernel::tessellate` actually emits for a
/// planar-faced solid (`kernel_real.rs:452`), i.e. what the production
/// surface→volume path receives BEFORE `RepairConfig`'s weld pre-stage runs.
fn unwelded_prismatic_box_mesh(lx: f32, ly: f32, lz: f32) -> Mesh {
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
// Shared assertion body
// ---------------------------------------------------------------------------

#[track_caller]
fn assert_rel(actual: f64, expected: f64, what: &str) {
    let err = (actual - expected).abs() / expected.abs().max(f64::MIN_POSITIVE);
    assert!(
        err <= REL,
        "{what}: got {actual:.12e}, expected {expected:.12e} \
         (relative error {err:.3e} > {REL:.3e})"
    );
}

/// Every completeness assertion #6200 turns on, for one already-meshed body.
///
/// `surface` is the mesh handed to the mesher; `label` names the fixture in
/// failure messages. The fill fraction is printed in every message so a
/// regression reports the actual coverage directly rather than a bare
/// volume mismatch.
#[track_caller]
fn assert_complete_prismatic_fill(report: &TetFillReport, surface: &Mesh, label: &str) {
    let surface_volume = enclosed_volume_of_surface(surface);
    let fill = report.aabb_fill_fraction();
    let match_ratio = report.surface_match_ratio(surface_volume);

    // 1. A complete conforming tetrahedralization of a convex polytope
    //    PARTITIONS it, and for an axis-aligned box the AABB of the
    //    tet-referenced nodes IS the box.
    assert_rel(
        report.abs_volume_sum,
        report.aabb_volume,
        &format!(
            "{label}: meshed volume vs AABB — aabb_fill_fraction = {fill:.6} \
             ({} tets, {} nodes). Pre-#6200 this read 0.74-0.86: gmsh classified \
             the box into 2 reparametrized patches instead of 6 planar faces, so \
             HXT filled a region smaller than the box.",
            report.n_tets, report.n_nodes
        ),
    );

    // 2. The geometry-agnostic reference: the volume enclosed by the INPUT
    //    surface. Unlike the AABB check this one is valid for any closed
    //    body, so it is the assertion that generalises beyond a box.
    assert_rel(
        report.abs_volume_sum,
        surface_volume,
        &format!(
            "{label}: meshed volume vs enclosed input-surface volume — \
             surface_match_ratio = {match_ratio:.6}"
        ),
    );

    // 3. No inverted elements.
    assert_eq!(
        report.inverted_tets, 0,
        "{label}: {} of {} tets are inverted (negative signed volume)",
        report.inverted_tets, report.n_tets
    );

    // 4. No orientation cancellation: with every element consistently wound,
    //    the signed sum must carry the full magnitude. Guards against a
    //    "correct total by coincidence" mesh where inverted elements cancel
    //    overlapping ones.
    assert_rel(
        report.signed_volume_sum.abs(),
        report.abs_volume_sum,
        &format!(
            "{label}: |signed volume sum| vs abs volume sum — a mismatch means \
             inverted elements are cancelling real volume"
        ),
    );
}

/// Mesh `surface` through the plain `mesh_to_volume` entry point.
///
/// Deliberately does NOT acquire `init::GMSH_LOCK`: `mesh_to_volume` acquires
/// it internally, so holding it here would self-deadlock (see the warning at
/// `mesh_plane_2d_tests.rs:22-23`).
fn fill_report_at(surface: &Mesh, mesh_size: Option<f64>, label: &str) -> TetFillReport {
    let options = MeshingOptions {
        mesh_size,
        ..MeshingOptions::default()
    };
    let kernel = GmshKernel::new();
    let volume = kernel
        .mesh_to_volume(surface, &options, ElementOrderTag::P1)
        .unwrap_or_else(|e| panic!("{label}: mesh_to_volume failed: {e:?}"));
    tet_fill_report(&volume)
        .unwrap_or_else(|| panic!("{label}: produced mesh is not measurable as tets"))
}

/// As [`fill_report_at`], at the auto-derived mesh size (the minimum triangle
/// edge, per `auto_size.rs`).
fn fill_report_for(surface: &Mesh, label: &str) -> TetFillReport {
    fill_report_at(surface, None, label)
}

// ---------------------------------------------------------------------------
// The acceptance guard, over three prismatic fixtures
// ---------------------------------------------------------------------------

/// The crate's own unit-cube fixture. Measured fill BEFORE the fix: 0.862916
/// (122 nodes / 91 tets / 0 interior nodes).
#[test]
fn unit_cube_is_completely_tetrahedralized() {
    let surface = prismatic_box_mesh(1.0, 1.0, 1.0);
    let report = fill_report_for(&surface, "unit cube 1x1x1");
    assert_complete_prismatic_fill(&report, &surface, "unit cube 1x1x1");
}

/// The post-repair shape the PRODUCTION path actually meshes for
/// `.ri box(1000mm, 100mm, 100mm)`: SI 1.0 x 0.1 x 0.1 m, welded to 8 vertices
/// / 12 triangles, auto mesh size 0.1 (the minimum triangle edge). Measured
/// fill BEFORE the fix: 0.736248 (220 nodes / 151 tets / 0 interior nodes).
#[test]
fn realized_production_box_is_completely_tetrahedralized() {
    let surface = prismatic_box_mesh(1.0, 0.1, 0.1);
    let report = fill_report_for(&surface, "box 1.0x0.1x0.1 m");
    assert_complete_prismatic_fill(&report, &surface, "box 1.0x0.1x0.1 m");
}

/// A shorter box at the same cross-section, to show the defect was not an
/// artefact of the 10:1 aspect ratio. Measured fill BEFORE the fix: 0.735705.
#[test]
fn short_box_is_completely_tetrahedralized() {
    let surface = prismatic_box_mesh(0.2, 0.1, 0.1);
    let report = fill_report_for(&surface, "box 0.2x0.1x0.1 m");
    assert_complete_prismatic_fill(&report, &surface, "box 0.2x0.1x0.1 m");
}

/// Scale invariance: the same body expressed in millimetres must mesh just as
/// completely. Pre-#6200 the defect was scale-invariant too (0.739724 at mm
/// scale vs 0.736248 at m scale), so this pins that the FIX is as well —
/// the feature angle is dimensionless, and nothing here may become
/// length-sensitive.
#[test]
fn millimetre_scale_box_is_completely_tetrahedralized() {
    let surface = prismatic_box_mesh(1000.0, 100.0, 100.0);
    let report = fill_report_for(&surface, "box 1000x100x100 mm");
    assert_complete_prismatic_fill(&report, &surface, "box 1000x100x100 mm");
}

// ---------------------------------------------------------------------------
// The exact production composition: unwelded OCCT-shaped input + repair
// ---------------------------------------------------------------------------

/// Drive the guard through `mesh_surface_to_volume_with_diagnostics` with an
/// UNWELDED 24-vertex input and `RepairConfig::default()`, i.e. the precise
/// composition `kernel_real.rs:464-478` uses in production, rather than only
/// the hand-welded input.
///
/// The reference volume is taken from the unwelded input directly: the
/// divergence-theorem sum is welding-independent (proven in
/// `tests/fill_metrics_tests.rs`), so it equals the repaired surface's volume
/// without needing the wrapper to hand back its intermediate.
#[test]
fn production_composition_unwelded_plus_repair_is_completely_tetrahedralized() {
    let surface = unwelded_prismatic_box_mesh(1.0, 0.1, 0.1);
    assert_eq!(
        surface.vertices.len() / 3,
        24,
        "fixture must be per-face unwelded, as OcctKernel::tessellate emits"
    );

    let report = mesh_surface_to_volume_with_diagnostics(
        &surface,
        &MeshingOptions::default(),
        ElementOrderTag::P1,
        Some(RepairConfig::default()),
        None,
        None,
    )
    .expect("mesh_surface_to_volume_with_diagnostics failed");

    let fill = tet_fill_report(&report.volume).expect("produced mesh is not measurable as tets");
    assert_complete_prismatic_fill(&fill, &surface, "unwelded box 1.0x0.1x0.1 m + repair");
}

// ---------------------------------------------------------------------------
// Interior nodes — resolution-driven, NOT a completeness signal
// ---------------------------------------------------------------------------

/// Interior nodes appear as a function of RESOLUTION vs cross-section, and are
/// independent of whether the decomposition is complete.
///
/// This test began life asserting `strictly_interior_nodes > 0` at the auto
/// mesh size, on the premise that a complete B-rep decomposition produces
/// interior nodes and that #6154's "0 interior nodes of 80" was therefore
/// diagnostic of #6200. **Measurement refutes that premise.** Sweeping mesh
/// size with the fix in place, fill fraction is 1.000000 in every row:
///
/// | body          | mesh size | nodes | tets | fill     | interior |
/// |---------------|-----------|-------|------|----------|----------|
/// | 1.0x0.1x0.1   | auto (0.1)|   440 |  258 | 1.000000 |        0 |
/// | 1.0x0.1x0.1   | 0.05      |   819 |  651 | 1.000000 |       11 |
/// | 1.0x0.1x0.1   | 0.025     |  2515 | 3505 | 1.000000 |      187 |
/// | 1x1x1         | auto (1.0)|   272 |  186 | 1.000000 |        0 |
/// | 1x1x1         | 0.5       |   273 |  194 | 1.000000 |        1 |
/// | 1x1x1         | 0.25      |   424 |  392 | 1.000000 |       12 |
///
/// A complete mesh can legitimately have ZERO interior nodes: the auto mesh
/// size is the minimum triangle edge (0.1 here), which makes the box's
/// cross-section exactly one element wide, so every node necessarily lands on
/// a face. Interior-node count therefore never discriminated #6200 — the fill
/// fraction did. (Consistently, the pre-fix sweep also showed 0 interior nodes
/// at h=0.1 and 4 at h=0.05, i.e. the same resolution dependence under the
/// bug.)
///
/// What IS worth pinning, and what this test now pins, is the conjunction: at a
/// resolution finer than the cross-section a complete mesh acquires interior
/// nodes, AND stays exactly full at every resolution. The second half is a
/// strictly stronger completeness guard than the original single-resolution
/// check, since a decomposition that bounded a smaller region would under-fill
/// at all three.
#[test]
fn interior_nodes_appear_once_resolution_is_finer_than_the_cross_section() {
    let surface = prismatic_box_mesh(1.0, 0.1, 0.1);

    // Coarse: cross-section exactly one element wide — zero interior nodes is
    // the CORRECT answer here, and the mesh is nonetheless complete.
    let coarse = fill_report_for(&surface, "box 1.0x0.1x0.1 m @ auto");
    assert_complete_prismatic_fill(&coarse, &surface, "box 1.0x0.1x0.1 m @ auto");

    // Finer than the cross-section: interior nodes must now exist, and the
    // mesh must STILL be exactly full.
    for size in [0.05f64, 0.025] {
        let report = fill_report_at(&surface, Some(size), &format!("box 1.0x0.1x0.1 m @ {size}"));
        assert_complete_prismatic_fill(&report, &surface, &format!("box 1.0x0.1x0.1 m @ {size}"));
        assert!(
            report.strictly_interior_nodes > 0,
            "box 1.0x0.1x0.1 m @ mesh size {size}: {} of {} nodes are strictly \
             interior (expected > 0). At a resolution finer than the 0.1 \
             cross-section a complete tet mesh must carry interior nodes; zero \
             would mean gmsh meshed only a boundary shell.",
            report.strictly_interior_nodes,
            report.n_nodes
        );
        assert!(
            report.strictly_interior_nodes > coarse.strictly_interior_nodes,
            "box 1.0x0.1x0.1 m: refining to {size} did not increase the interior-node \
             count ({} at auto size vs {} at {size})",
            coarse.strictly_interior_nodes,
            report.strictly_interior_nodes
        );
    }
}
