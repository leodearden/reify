//! Geometry contract for the workspace-canonical box / cylinder fixtures
//! hoisted into [`reify_test_support::fixtures`] by task #6387.
//!
//! # What this pins, and what it deliberately does not
//!
//! These fixtures are the shared input to every signed-volume, fill-fraction
//! and divergence-theorem guard in `reify-kernel-gmsh`. Once the geometry lives
//! in one place, a silent edit here would move all of those guards at once — so
//! the fixtures need their own contract, independent of any consumer.
//!
//! Everything asserted below is RUNTIME GEOMETRY: buffer shapes, enclosed
//! volumes against independent closed forms, and the tolerance helper's own
//! boundary behaviour. Nothing here asserts on doc-comment text, function names
//! or module prose — that would pin the writing rather than the fixture.
//!
//! # Why the divergence-theorem helper is local
//!
//! `enclosed_volume_of_surface` already exists in
//! `reify-kernel-gmsh/src/fill_metrics.rs`, but `reify-test-support` must not
//! take a dependency on a kernel adapter to test its own fixtures. The
//! twenty-line reimplementation below is the deliberate cost of keeping that
//! edge absent; being an independent second implementation is a mild bonus.

use reify_ir::Mesh;
use reify_test_support::fixtures::{
    F32_STORAGE_REL, assert_rel, prismatic_box_mesh, tessellated_cylinder_mesh,
    tessellated_cylinder_volume, unwelded_prismatic_box_mesh,
};
use reify_test_support::helpers::mesh_aabb;

/// Enclosed volume of a closed, outward-wound triangle surface, by the
/// divergence theorem: `sum over triangles of v0 · ((v1 - v0) × (v2 - v0)) / 6`.
///
/// Coordinates are widened to f64 and the sum is accumulated in f64, so the
/// only f32 error in the result is the storage error already baked into the
/// fixture's vertices — which is exactly what [`F32_STORAGE_REL`] budgets for.
///
/// The sum is a per-triangle integral against the origin, so it is
/// welding-independent: a welded and an unwelded encoding of the same surface
/// must produce the same number.
fn enclosed_volume(mesh: &Mesh) -> f64 {
    assert!(
        mesh.indices.len().is_multiple_of(3),
        "enclosed_volume: index buffer is not a whole number of triangles"
    );
    let node = |i: u32| -> [f64; 3] {
        let i = i as usize;
        [
            mesh.vertices[3 * i] as f64,
            mesh.vertices[3 * i + 1] as f64,
            mesh.vertices[3 * i + 2] as f64,
        ]
    };
    let mut sum = 0.0f64;
    for tri in mesh.indices.chunks_exact(3) {
        let (a, b, c) = (node(tri[0]), node(tri[1]), node(tri[2]));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        sum += a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2];
    }
    sum / 6.0
}

// ---------------------------------------------------------------------------
// Welded box
// ---------------------------------------------------------------------------

/// The welded box has 8 vertices / 12 triangles and encloses exactly
/// `lx * ly * lz`.
///
/// `1.0 x 0.1 x 0.1` is chosen because it is the case #6154 independently
/// measured: 0.1 is not representable in binary, so the fixture stores
/// 0.100000001490116… and the body measures 1.0000000298e-2 against an exact
/// 1e-2 — relative 2.98e-8, which [`F32_STORAGE_REL`]'s 1e-6 band clears ~30x.
#[test]
fn prismatic_box_has_eight_vertices_twelve_triangles_and_exact_volume() {
    let mesh = prismatic_box_mesh(1.0, 0.1, 0.1);

    assert_eq!(mesh.vertices.len(), 24, "8 vertices x 3 components");
    assert_eq!(mesh.indices.len(), 36, "12 triangles x 3 indices");
    assert!(mesh.normals.is_none());

    assert_rel(
        enclosed_volume(&mesh),
        1.0 * 0.1 * 0.1,
        F32_STORAGE_REL,
        "enclosed volume of prismatic_box_mesh(1.0, 0.1, 0.1)",
    );
}

// ---------------------------------------------------------------------------
// Unwelded box
// ---------------------------------------------------------------------------

/// The per-face unwelded box carries 24 vertices (each face its own 4 corner
/// copies) and the same 12 triangles, and must enclose the identical volume.
///
/// This is the shape `OcctKernel::tessellate` actually emits for a
/// planar-faced solid — i.e. what the production surface→volume path receives
/// BEFORE `RepairConfig`'s weld pre-stage. If the two encodings ever disagreed
/// on volume, every guard that compares a repaired body against an unrepaired
/// one would be measuring the fixture rather than the pipeline.
#[test]
fn unwelded_box_has_24_vertices_and_encloses_the_welded_volume() {
    let unwelded = unwelded_prismatic_box_mesh(1.0, 0.1, 0.1);

    assert_eq!(unwelded.vertices.len(), 72, "24 vertices x 3 components");
    assert_eq!(unwelded.indices.len(), 36, "12 triangles x 3 indices");
    assert!(unwelded.normals.is_none());

    let welded_volume = enclosed_volume(&prismatic_box_mesh(1.0, 0.1, 0.1));
    assert_rel(
        enclosed_volume(&unwelded),
        welded_volume,
        F32_STORAGE_REL,
        "enclosed volume of the unwelded box vs the welded box",
    );
}

// ---------------------------------------------------------------------------
// Tessellated cylinder
// ---------------------------------------------------------------------------

/// The cylinder fixture's buffer shape, enclosed volume and XY extent.
///
/// The volume bound is not invented: `volume_fill_fraction.rs`'s
/// `tessellated_cylinder_is_completely_tetrahedralized` already asserts this
/// exact fixture against this exact closed form at this exact 1e-6 tolerance,
/// and passes on main. Checking the fixture against an INDEPENDENT closed form
/// (a prism over an inscribed regular n-gon — not `pi r^2 h`) means a fixture
/// bug cannot quietly move the target that consumers compare meshes against.
#[test]
fn tessellated_cylinder_shape_volume_and_extent() {
    const R: f64 = 0.5;
    const H: f64 = 1.0;
    const N: usize = 24;

    let mesh = tessellated_cylinder_mesh(R as f32, H as f32, N);

    // 2n rim vertices + one centre vertex per cap.
    assert_eq!(mesh.vertices.len(), (2 * N + 2) * 3, "2n + 2 = 50 vertices");
    // 2n lateral + n per cap.
    assert_eq!(mesh.indices.len(), 4 * N * 3, "4n = 96 triangles");
    assert!(mesh.normals.is_none());

    assert_rel(
        enclosed_volume(&mesh),
        tessellated_cylinder_volume(R, H, N),
        F32_STORAGE_REL,
        "enclosed volume of the cylinder fixture vs its closed form",
    );

    // PRECONDITION for the exact-extent claim below: `N % 4 == 0`. Vertex 0
    // sits at angle 0 and the ring is walked in equal 360/n steps, so the ring
    // lands on all four axis directions (0°, 90°, 180°, 270°) only when 4
    // divides n — merely-even n gives 0° and 180° but not 90°/270°, and the Y
    // extent then falls short of r by the tessellation shortfall
    // `r * (1 - cos(pi/n))`. Do not copy these assertions to an n that is not a
    // multiple of 4; assert a shortfall-tolerant bound there instead.
    const _: () = assert!(
        N.is_multiple_of(4),
        "exact 2r x 2r XY extent requires n % 4 == 0"
    );
    let (min, max) = mesh_aabb(&mesh);
    assert_eq!(min[0], -(R as f32), "cylinder min x");
    assert_eq!(max[0], R as f32, "cylinder max x");
    assert_eq!(min[1], -(R as f32), "cylinder min y");
    assert_eq!(max[1], R as f32, "cylinder max y");
    assert_eq!(min[2], 0.0, "cylinder base sits on z = 0");
    assert_eq!(max[2], H as f32, "cylinder top sits on z = h");
}

/// The mesh still matches its closed form in the COARSE regime, where the
/// n-gon prism is furthest from the circular cylinder it approximates.
///
/// The fixture's doc comment makes small `n` the load-bearing case — that is
/// where the lateral facet normals turn by more than the 45° feature-angle
/// threshold, which is the whole reason the segment count is a parameter. It is
/// also where a WRONG closed form is easiest to catch: at `n = 6` the inscribed
/// hexagon holds only `3*sqrt(3)/(2*pi)` ~ 0.827 of the circle's area, so a
/// `pi r^2 h` stand-in would miss by ~17% — a gap the smooth `n = 24` case
/// above (~0.9886, a 1.1% gap) is far weaker at separating.
///
/// Volume only: `n = 6` is not a multiple of 4, so the exact `2r x 2r` XY
/// extent asserted above does NOT hold here (the Y extent falls short by
/// `r * (1 - cos(pi/6))`), and `n = 5` additionally breaks the `2r` X extent.
#[test]
fn tessellated_cylinder_matches_its_closed_form_at_coarse_segment_counts() {
    const R: f64 = 0.5;
    const H: f64 = 1.0;

    for n in [5usize, 6, 8] {
        let mesh = tessellated_cylinder_mesh(R as f32, H as f32, n);
        assert_eq!(mesh.vertices.len(), (2 * n + 2) * 3, "2n + 2 vertices");
        assert_eq!(mesh.indices.len(), 4 * n * 3, "4n triangles");
        assert_rel(
            enclosed_volume(&mesh),
            tessellated_cylinder_volume(R, H, n),
            F32_STORAGE_REL,
            &format!("enclosed volume of the n = {n} cylinder vs its closed form"),
        );
    }
}

/// The fixture's own `n >= 3` precondition is a hard assert, not a silent
/// degenerate mesh.
#[test]
#[should_panic(expected = "at least 3 segments")]
fn tessellated_cylinder_rejects_fewer_than_three_segments() {
    let _ = tessellated_cylinder_mesh(0.5, 1.0, 2);
}

// ---------------------------------------------------------------------------
// The tolerance helper itself
// ---------------------------------------------------------------------------

// [`F32_STORAGE_REL`] deliberately has no `assert_eq!(F32_STORAGE_REL, 1e-6)`
// test. Restating a constant's literal value cannot detect a defect: any edit
// to the constant fails such a test mechanically and the fix is to edit the
// test, which adds a step rather than protection. The tolerance is already
// load-bearing in the three geometry tests above — each measures a real
// enclosed volume against an independent closed form through this band — so
// loosening it enough to matter reds them, and tightening it below the ~3.6e-7
// f32 storage ceiling derived in its doc comment reds them too.

/// `assert_rel` is INCLUSIVE at its boundary: `err == rel` passes.
///
/// Dyadic values throughout, so the arithmetic is exact and this is a statement
/// about the comparison operator rather than about rounding: `|1.5 - 1.0| / 1.0`
/// is exactly 0.5.
#[test]
fn assert_rel_accepts_error_exactly_equal_to_the_tolerance() {
    assert_rel(1.5, 1.0, 0.5, "boundary case");
}

/// One ULP past the boundary panics — pinning `<=` rather than `<`.
///
/// `nextafter(1.5)` is `1.5 + 2^-52`; subtracting 1.0 gives exactly
/// `0.5 + 2^-52`, which is representable, so the computed error is strictly
/// greater than 0.5 by construction.
#[test]
#[should_panic(expected = "relative error")]
fn assert_rel_rejects_error_one_ulp_past_the_tolerance() {
    let just_over = f64::from_bits(1.5f64.to_bits() + 1);
    assert_rel(just_over, 1.0, 0.5, "one ulp outside");
}

/// A zero `expected` does not divide by zero, and `0.0 == 0.0` passes even at
/// `rel = 0.0`.
///
/// `assert_rel`'s denominator is `expected.abs().max(f64::MIN_POSITIVE)`, which
/// is the single non-obvious line in the helper. Pinning it matters because
/// consumers really do hand it near-zero expectations — `fill_metrics_tests.rs`
/// carries `surface_match_ratio_is_finite_for_a_zero_reference_volume` and
/// `empty_surface_encloses_zero_volume` — so this is a contract, not a
/// hypothetical. Here the numerator is exactly 0, so `0 / MIN_POSITIVE` is 0
/// and any tolerance clears it.
#[test]
fn assert_rel_accepts_an_exact_zero_against_a_zero_expectation() {
    assert_rel(0.0, 0.0, 0.0, "exact zero");
}

/// The other side of that guard: against `expected == 0.0` the denominator is
/// `MIN_POSITIVE` rather than the numerator's own magnitude, so ANY non-zero
/// `actual` is an astronomically large RELATIVE error and panics.
///
/// `f64::MIN_POSITIVE` is the smallest NORMAL double, 2.225e-308, so `1e-300`
/// — minuscule in absolute terms — yields a relative error of ~4.5e7, thirteen
/// orders of magnitude outside the 1e-6 band. The consequence worth knowing at a call
/// site: `assert_rel` against a zero expectation is an exact-equality check,
/// not a tolerance check — a near-zero result needs an absolute bound instead.
#[test]
#[should_panic(expected = "relative error")]
fn assert_rel_rejects_any_non_zero_actual_against_a_zero_expectation() {
    assert_rel(1e-300, 0.0, F32_STORAGE_REL, "near-zero against exact zero");
}
