//! Acceptance guard: `mesh_to_volume` must produce a COMPLETE
//! tetrahedralization of the body it is handed (#6200).
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
//!
//! # Curved geometry
//!
//! Most fixtures here are prismatic, because that is where the defect was
//! measured and where `aabb_fill_fraction` is a valid invariant. But
//! `CLASSIFY_FEATURE_ANGLE` is a GLOBAL parameter — lowering it makes gmsh
//! split MORE, so it also changes how curved bodies are decomposed.
//! `tessellated_cylinder_is_completely_tetrahedralized` covers that half,
//! against the geometry-agnostic `surface_match_ratio` rather than the AABB.

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
//
// Shared with `tests/fill_metrics_tests.rs` and
// `tests/classify_feature_angle.rs` through `tests/common/mod.rs`, so the three
// views of #6200 (arithmetic / symptom / mechanism) cannot drift apart on what
// "a box" is.

mod common;
use common::{
    prismatic_box_mesh, tessellated_cylinder_mesh, tessellated_cylinder_volume,
    unwelded_prismatic_box_mesh,
};

// ---------------------------------------------------------------------------
// Shared assertion body
// ---------------------------------------------------------------------------

/// [`common::assert_rel`] bound to this file's derived [`REL`] tolerance.
///
/// A thin binding, not a second implementation: the comparison itself lives in
/// exactly one place. A call site needing a different band (the curved fixture
/// below) calls `common::assert_rel` directly with its own stated tolerance,
/// rather than loosening `REL` for everyone.
#[track_caller]
fn assert_rel(actual: f64, expected: f64, what: &str) {
    common::assert_rel(actual, expected, REL, what);
}

/// Every completeness assertion #6200 turns on, for one already-meshed body.
///
/// `surface` is the mesh handed to the mesher; `label` names the fixture in
/// failure messages. The fill fraction is printed in every message so a
/// regression reports the actual coverage directly rather than a bare
/// volume mismatch.
#[track_caller]
fn assert_complete_prismatic_fill(report: &TetFillReport, surface: &Mesh, label: &str) {
    let surface_volume = enclosed_volume_of_surface(surface).unwrap_or_else(|| {
        panic!("{label}: the input surface is not measurable as a triangle soup")
    });
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
// Curved geometry — the non-prismatic half of the constant's blast radius
// ---------------------------------------------------------------------------

/// Cylinder fixture: radius, height, tessellation segments.
const CYL_R: f64 = 0.5;
const CYL_H: f64 = 1.0;
const CYL_SEGMENTS: usize = 24;

/// The two mesh sizes the curved guard meshes at.
///
/// Both are COARSER than the fixture's auto size (0.1305, the rim edge) so the
/// pair costs less wall-clock than the single auto-size reading it replaces.
const CYL_COARSE: f64 = 0.25;
const CYL_FINE: f64 = 0.13;

/// Completeness band for the curved fixture at [`CYL_FINE`].
///
/// Deliberately four orders looser than [`REL`], and for a different reason.
/// [`REL`] budgets f32 coordinate storage only, which is the whole error budget
/// for a box: gmsh's B-rep decomposition of a planar-faced solid reproduces the
/// input faces exactly, so the meshed volume is the input volume up to storage.
/// A curved body carries a second, much larger term. `create_geometry`
/// reparametrizes the barrel and `mesh_generate(3)` lays a NEW surface mesh on
/// that parametrization, whose cross-section is inscribed inside the input
/// polygon — so the meshed solid is slightly smaller than the input polyhedron
/// by a chord residual, not by round-off.
///
/// Derived from the measured residual, not tuned to it. Measured
/// `1 - surface_match_ratio` on this branch (n = 12 / 24 / 48 segments, so the
/// residual is a function of the MESH size, essentially not of the fixture's
/// tessellation density):
///
/// | mesh size | n=12     | n=24     | n=48     |
/// |-----------|----------|----------|----------|
/// | 0.25      | 2.14e-2  | 2.31e-2  | 2.46e-2  |
/// | 0.13      | 5.76e-3  | 6.19e-3  | 6.85e-3  |
/// | 0.09      | 2.73e-3  | 2.94e-3  | 3.18e-3  |
///
/// i.e. ~h^2, as a chord residual must be. 1.5e-2 clears the 6.19e-3 reading at
/// [`CYL_FINE`] by ~2.4x while still sitting ~17x below the ~26% defect #6200
/// guards against.
const CURVED_REL: f64 = 1.5e-2;

/// A closed CURVED body must be completely tetrahedralized too.
///
/// `CLASSIFY_FEATURE_ANGLE` is a GLOBAL production parameter: #6200's
/// `π/2 → π/4` change alters the B-rep decomposition of every body
/// `mesh_to_volume` meshes, not just boxes. Every other fixture in this file
/// and in `tests/classify_feature_angle.rs` is axis-aligned and prismatic, so
/// nothing here covered the direction the change actually makes *more*
/// aggressive: a lower threshold splits MORE, and a coarsely-tessellated curved
/// face (a low-segment fillet, a small hole) can now be decomposed into many
/// patches that `create_geometry` must reparametrize individually. On this
/// fixture the two cap rims are genuine 90° feature edges — registered as sharp
/// only AFTER the fix — while the barrel's 15°-per-facet turn stays smooth.
///
/// This is also the only test that exercises `surface_match_ratio` on the
/// geometry it was built for. The module docs advertise it as "the
/// geometry-agnostic one… the ratio worth reporting on a production path",
/// while `aabb_fill_fraction` is documented prismatic-only — a cylinder
/// legitimately reads ≈ π/4. Both halves of that claim are checked below, so
/// the docs are pinned by measurement rather than asserted in prose.
///
/// # Why two resolutions
///
/// A single loose-tolerance reading cannot tell a *discretization residual*
/// (harmless, converges away) from an *incomplete decomposition* (#6200, which
/// does not). The pre-#6200 box refined to only 0.926 fill at 4x resolution —
/// the defect stayed put under refinement. So the guard pins CONVERGENCE as
/// well as magnitude: halving the mesh size must shrink the residual severalfold.
#[test]
fn tessellated_cylinder_is_completely_tetrahedralized() {
    let label = "cylinder r=0.5 h=1.0 x24";
    let surface = tessellated_cylinder_mesh(CYL_R as f32, CYL_H as f32, CYL_SEGMENTS);
    let surface_volume =
        enclosed_volume_of_surface(&surface).expect("cylinder fixture must be well-formed");

    // The reference itself is first checked against a CLOSED FORM, so a fixture
    // bug cannot quietly move the target the mesh is compared against. A prism
    // over an INSCRIBED regular n-gon — not pi*r^2*h.
    let closed_form = tessellated_cylinder_volume(CYL_R, CYL_H, CYL_SEGMENTS);
    assert_rel(
        surface_volume,
        closed_form,
        "enclosed volume of the cylinder fixture vs its closed form",
    );

    let coarse = fill_report_at(&surface, Some(CYL_COARSE), label);
    let fine = fill_report_at(&surface, Some(CYL_FINE), label);

    for (report, size) in [(&coarse, CYL_COARSE), (&fine, CYL_FINE)] {
        // No inverted elements, and no orientation cancellation, at either
        // resolution. Unlike the volume totals these are exact properties —
        // the chord residual cannot excuse a negatively-wound element.
        assert_eq!(
            report.inverted_tets, 0,
            "{label} @ {size}: {} of {} tets are inverted (negative signed volume)",
            report.inverted_tets, report.n_tets
        );
        common::assert_rel(
            report.signed_volume_sum.abs(),
            report.abs_volume_sum,
            REL,
            &format!(
                "{label} @ {size}: |signed volume sum| vs abs volume sum — a mismatch \
                 means inverted elements are cancelling real volume"
            ),
        );
        // The prismatic-only caveat, demonstrated rather than asserted in prose:
        // this body is COMPLETE and still reads well below 1.0 here. A fill
        // floor on the production path would reject it, which is why
        // `mesh_surface_to_volume_with_diagnostics` reports and never enforces.
        let fill = report.aabb_fill_fraction();
        assert!(
            (0.70..0.85).contains(&fill),
            "{label} @ {size}: aabb_fill_fraction = {fill:.6}, expected ~0.7765 (the \
             inscribed 24-gon's area over its 2r x 2r bounding square; pi/4 ~ 0.7854 \
             in the smooth limit). This ratio is PRISMATIC-ONLY — a value near 1.0 \
             here would mean the fixture stopped being curved, not that the mesh \
             improved."
        );
    }

    // 1. The geometry-agnostic completeness check — the one assertion in this
    //    file valid for a curved or non-convex body.
    let fine_ratio = fine.surface_match_ratio(surface_volume);
    common::assert_rel(
        fine.abs_volume_sum,
        surface_volume,
        CURVED_REL,
        &format!(
            "{label} @ {CYL_FINE}: meshed volume vs enclosed input-surface volume — \
             surface_match_ratio = {fine_ratio:.6} ({} tets, {} nodes)",
            fine.n_tets, fine.n_nodes
        ),
    );

    // 2. CONVERGENCE — what separates a chord residual from an under-fill.
    //    Measured 2.31e-2 -> 6.19e-3, a factor of 3.73 for a mesh-size ratio of
    //    1.92 (i.e. ~h^2). The bar is 2x, well inside that, but far outside the
    //    flat-under-refinement behaviour of the #6200 defect itself.
    let coarse_residual = (1.0 - coarse.surface_match_ratio(surface_volume)).abs();
    let fine_residual = (1.0 - fine_ratio).abs();
    assert!(
        fine_residual * 2.0 < coarse_residual,
        "{label}: refining {CYL_COARSE} -> {CYL_FINE} shrank the completeness residual \
         only from {coarse_residual:.3e} to {fine_residual:.3e} (< 2x). A chord \
         residual converges as ~h^2; a residual that does NOT converge is an \
         incomplete decomposition, which is #6200 — the pre-fix box still read \
         0.926 fill at 4x resolution."
    );

    // 3. The AABB ratio tracks the fixture's own closed-form area ratio, so the
    //    prismatic-only reading is not merely "some number below 1".
    common::assert_rel(
        fine.aabb_fill_fraction(),
        closed_form / (2.0 * CYL_R * 2.0 * CYL_R * CYL_H),
        CURVED_REL,
        &format!("{label} @ {CYL_FINE}: aabb_fill_fraction vs the n-gon's area ratio"),
    );
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

/// The production entry point must SURFACE the fill report, not merely permit
/// one to be computed after the fact.
///
/// `fill_metrics`' module docs state that
/// `mesh_surface_to_volume_with_diagnostics` "surfaces the report non-fatally
/// through `MeshSurfaceToVolumeReport::fill`". This test is what makes that
/// sentence checkable. A doc asserting a property the code does not have is
/// precisely the failure mode that CAUSED #6200 — the old `classify_surfaces`
/// comment claimed a B-rep split that never happened, and nothing tested it —
/// so the claim is closed by pinning it, not by deleting it.
///
/// Asserts only that the field is POPULATED and CONSISTENT. No threshold is
/// asserted against production code: per the module docs, no fill floor is
/// simultaneously scale-free and shape-free, so a guessed constant would reject
/// legitimate curved geometry. The 1.0 check below is a property of *this
/// prismatic fixture*, not a contract the producer enforces.
#[test]
fn production_report_surfaces_the_fill_measurement() {
    let surface = unwelded_prismatic_box_mesh(1.0, 0.1, 0.1);

    let report = mesh_surface_to_volume_with_diagnostics(
        &surface,
        &MeshingOptions::default(),
        ElementOrderTag::P1,
        Some(RepairConfig::default()),
        None,
        None,
    )
    .expect("mesh_surface_to_volume_with_diagnostics failed");

    // (1) Populated for a well-formed tet mesh.
    let fill = report
        .fill
        .expect("MeshSurfaceToVolumeReport::fill must be Some(_) for a well-formed tet mesh");

    // (2) It is the report for the mesh actually handed back — not a stale
    //     value, and not recomputed from some other intermediate.
    let recomputed =
        tet_fill_report(&report.volume).expect("produced mesh is not measurable as tets");
    assert_eq!(
        fill, recomputed,
        "report.fill must equal tet_fill_report(&report.volume); \
         a mismatch means the plumbed value describes a different mesh than the \
         one returned"
    );

    // (3) On this prismatic fixture the plumbed value reads a complete fill.
    assert_rel(
        fill.aabb_fill_fraction(),
        1.0,
        "production report fill.aabb_fill_fraction()",
    );
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
