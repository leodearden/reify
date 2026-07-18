//! Differential correctness tests locking single-pass pattern semantics
//! against independent references (task 5213, step-7).
//!
//! The single-pass n-ary fuse (`fuse_shape_list`) replaced the O(N²)
//! pairwise-accumulator loop in the four pattern realizers.  These tests pin
//! that the *observable output* — volume, surface area, watertightness, and
//! connectedness — is unchanged, exercising the tricky disjoint / overlapping /
//! single-instance edge cases the single-pass BOP must handle:
//!
//! - **Disjoint** grids/rows fuse to a watertight, multi-solid result whose
//!   total volume is the exact analytic sum (N × the unit-box volume) and whose
//!   surface area is the exact sum of every instance's faces (N × 6 m²).  If any
//!   two instances had spuriously merged, both the volume and the area would
//!   drop — so `area == N·6` is a direct "N separate solids, nothing merged"
//!   witness.  (`IsConnected` cannot serve here: `fuse_shape_list` rewraps a
//!   disjoint fuse's COMPOUND-of-solids as a COMPSOLID so it is
//!   watertight-queryable, and a COMPSOLID reports `IsConnected == true` by
//!   OCCT's connected-assembly definition — see `conformance_integration.rs`.)
//! - **Overlapping** grids/rows fuse to a watertight *single* solid
//!   (`IsConnected == true`) whose volume equals an independent reference — for
//!   the 2-D grid a chained binary `boolean_fuse` of the same placed instances;
//!   for the analytically-simple 1-D / arbitrary cases the exact bounding-span
//!   volume.  This is the "union, not compound" witness: a bare compound would
//!   double-count the overlaps (larger volume) and report multiple components.
//!
//! All shapes are built in SI metres.  `GeometryOp::Box` is centred at the
//! origin, so a unit (1 m) box spans [-0.5, +0.5] on each axis; `Translate`
//! yields a fresh, independent handle (source unchanged).  `linear_pattern_2d`
//! places grid instance (i, j) at `i·spacing1·dir1 + j·spacing2·dir2` with the
//! original at (0, 0); `arbitrary_pattern` realizes the original plus one copy
//! per transform (so K transforms → K+1 instances).
//!
//! `#![cfg(has_occt)]` gates the whole file to hosts with OCCT — the same gate
//! every kernel integration-test file in this crate uses.

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// Relative tolerance for volume / area comparisons.  OCCT boolean volume and
/// surface-area numerics are exact to well within 1e-6 for these axis-aligned
/// box unions.
const REL_TOL: f64 = 1e-6;

fn within_rel(actual: f64, expected: f64, tol: f64) -> bool {
    (actual - expected).abs() <= tol * expected.abs().max(1.0)
}

/// Build a fresh 1 m unit cube centred at the origin; return its handle id.
fn unit_box(kernel: &mut OcctKernel) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(1.0),
            height: Value::Real(1.0),
            depth: Value::Real(1.0),
        })
        .expect("unit box creation should succeed")
        .id
}

/// Translate `src` by `(dx, dy, dz)` metres, returning a fresh handle id.
fn translated(
    kernel: &mut OcctKernel,
    src: GeometryHandleId,
    dx: f64,
    dy: f64,
    dz: f64,
) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Translate {
            target: src,
            dx,
            dy,
            dz,
        })
        .expect("translate should succeed")
        .id
}

/// Query the (positive) volume of a handle in m³.
fn volume_of(kernel: &mut OcctKernel, id: GeometryHandleId) -> f64 {
    let v = kernel
        .query(&GeometryQuery::Volume(id))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume should be numeric");
    assert!(v > 0.0, "volume must be positive, got {v}");
    v
}

/// Query the (positive) surface area of a handle in m².
fn surface_area_of(kernel: &mut OcctKernel, id: GeometryHandleId) -> f64 {
    let a = kernel
        .query(&GeometryQuery::SurfaceArea(id))
        .expect("surface-area query should succeed")
        .as_f64()
        .expect("surface area should be numeric");
    assert!(a > 0.0, "surface area must be positive, got {a}");
    a
}

/// Query whether a handle is watertight.
fn is_watertight(kernel: &OcctKernel, id: GeometryHandleId) -> bool {
    match kernel.query(&GeometryQuery::IsWatertight(id)) {
        Ok(Value::Bool(b)) => b,
        other => panic!("IsWatertight should return Ok(Bool(_)), got {other:?}"),
    }
}

/// Query whether a handle is a single connected component.
fn is_connected(kernel: &OcctKernel, id: GeometryHandleId) -> bool {
    match kernel.query(&GeometryQuery::IsConnected(id)) {
        Ok(Value::Bool(b)) => b,
        other => panic!("IsConnected should return Ok(Bool(_)), got {other:?}"),
    }
}

/// Build an independent reference for a `direction1=+X`, `direction2=+Y` grid by
/// placing a fresh unit box at each grid position (i·s1, j·s2, 0) — matching
/// `linear_pattern_2d`'s placement, original at (0, 0) — and chain-fusing them
/// with binary `Union`.  Same geometry, different code path (N−1 pairwise fuses
/// vs. one n-ary fuse), so the union volumes must agree to float/mesh noise.
fn chained_grid_reference_x_y(
    kernel: &mut OcctKernel,
    count1: u32,
    count2: u32,
    spacing1: f64,
    spacing2: f64,
) -> GeometryHandleId {
    let mut acc: Option<GeometryHandleId> = None;
    for i in 0..count1 {
        for j in 0..count2 {
            let b = unit_box(kernel);
            let placed = translated(
                kernel,
                b,
                f64::from(i) * spacing1,
                f64::from(j) * spacing2,
                0.0,
            );
            acc = Some(match acc {
                None => placed,
                Some(a) => kernel
                    .execute(&GeometryOp::Union {
                        left: a,
                        right: placed,
                    })
                    .expect("reference chained union must succeed")
                    .id,
            });
        }
    }
    acc.expect("grid must have at least one instance")
}

// ── linear_pattern_2d ──────────────────────────────────────────────────────────

/// DISJOINT 3×3 grid (spacing 2 m > 1 m box): watertight, volume == 9.0 m³, and
/// surface area == 54.0 m² (9 boxes × 6 m², no faces merged ⇒ 9 separate solids).
#[test]
fn linear_pattern_2d_disjoint_3x3_volume_area_watertight() {
    let mut kernel = OcctKernel::new();
    let b = unit_box(&mut kernel);

    let grid = kernel
        .execute(&GeometryOp::LinearPattern2D {
            target: b,
            direction1: [1.0, 0.0, 0.0],
            count1: 3,
            spacing1: Value::Real(2.0),
            direction2: [0.0, 1.0, 0.0],
            count2: 3,
            spacing2: Value::Real(2.0),
        })
        .expect("disjoint 3x3 linear_pattern_2d must succeed")
        .id;

    let vol = volume_of(&mut kernel, grid);
    assert!(
        within_rel(vol, 9.0, REL_TOL),
        "a disjoint 3x3 grid of unit boxes must have volume 9.0 m³, got {vol:.9}"
    );
    let area = surface_area_of(&mut kernel, grid);
    assert!(
        within_rel(area, 54.0, REL_TOL),
        "9 disjoint unit boxes must have total surface area 54.0 m² (nothing merged), got {area:.9}"
    );
    assert!(
        is_watertight(&kernel, grid),
        "the disjoint multi-solid grid result must be watertight"
    );
}

/// OVERLAPPING 3×3 grid (spacing 0.5 m < 1 m box): watertight single component,
/// volume equal to a chained-`boolean_fuse` reference over the same 9 placed
/// instances (proves union — overlaps merged — not a volume-double-counting
/// compound).
#[test]
fn linear_pattern_2d_overlapping_matches_chained_reference_single_component() {
    let mut kernel = OcctKernel::new();

    // Independent reference: chained binary fuse of the same 9 placed instances.
    let reference = chained_grid_reference_x_y(&mut kernel, 3, 3, 0.5, 0.5);
    let ref_vol = volume_of(&mut kernel, reference);

    let b = unit_box(&mut kernel);
    let grid = kernel
        .execute(&GeometryOp::LinearPattern2D {
            target: b,
            direction1: [1.0, 0.0, 0.0],
            count1: 3,
            spacing1: Value::Real(0.5),
            direction2: [0.0, 1.0, 0.0],
            count2: 3,
            spacing2: Value::Real(0.5),
        })
        .expect("overlapping 3x3 linear_pattern_2d must succeed")
        .id;

    let vol = volume_of(&mut kernel, grid);
    assert!(
        within_rel(vol, ref_vol, REL_TOL),
        "overlapping grid volume {vol:.9} must equal the chained-fuse reference {ref_vol:.9}"
    );
    assert!(
        is_watertight(&kernel, grid),
        "the overlapping single-solid grid result must be watertight"
    );
    assert!(
        is_connected(&kernel, grid),
        "overlapping instances must fuse into ONE connected component, not a compound"
    );
}

// ── arbitrary_pattern ──────────────────────────────────────────────────────────

/// DISJOINT arbitrary_pattern: original + 2 far translations (@3 m, @6 m) ⇒ 3
/// disjoint boxes ⇒ watertight, volume 3.0 m³, surface area 18.0 m².
#[test]
fn arbitrary_pattern_disjoint_volume_area_watertight() {
    let mut kernel = OcctKernel::new();
    let b = unit_box(&mut kernel);

    let pat = kernel
        .execute(&GeometryOp::ArbitraryPattern {
            target: b,
            transforms: vec![
                ([1.0, 0.0, 0.0, 0.0], [3.0, 0.0, 0.0]),
                ([1.0, 0.0, 0.0, 0.0], [6.0, 0.0, 0.0]),
            ],
        })
        .expect("disjoint arbitrary_pattern must succeed")
        .id;

    let vol = volume_of(&mut kernel, pat);
    assert!(
        within_rel(vol, 3.0, REL_TOL),
        "original + 2 disjoint copies must have volume 3.0 m³, got {vol:.9}"
    );
    let area = surface_area_of(&mut kernel, pat);
    assert!(
        within_rel(area, 18.0, REL_TOL),
        "3 disjoint unit boxes must have surface area 18.0 m² (nothing merged), got {area:.9}"
    );
    assert!(
        is_watertight(&kernel, pat),
        "the disjoint arbitrary_pattern result must be watertight"
    );
}

/// OVERLAPPING arbitrary_pattern: original [-0.5,0.5] + copies @0.5 ([0,1]) and
/// @1.0 ([0.5,1.5]) ⇒ union spans [-0.5,1.5] on X ⇒ volume 2.0 m³ exactly (a
/// bare compound would be 3.0), watertight, single connected component.
#[test]
fn arbitrary_pattern_overlapping_merges_single_component() {
    let mut kernel = OcctKernel::new();
    let b = unit_box(&mut kernel);

    let pat = kernel
        .execute(&GeometryOp::ArbitraryPattern {
            target: b,
            transforms: vec![
                ([1.0, 0.0, 0.0, 0.0], [0.5, 0.0, 0.0]),
                ([1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
        })
        .expect("overlapping arbitrary_pattern must succeed")
        .id;

    let vol = volume_of(&mut kernel, pat);
    assert!(
        within_rel(vol, 2.0, REL_TOL),
        "overlapping copies must fuse to volume 2.0 m³ (union, not 3.0 compound), got {vol:.9}"
    );
    assert!(
        is_watertight(&kernel, pat),
        "the overlapping arbitrary_pattern result must be watertight"
    );
    assert!(
        is_connected(&kernel, pat),
        "overlapping copies must fuse into ONE connected component"
    );
}

// ── linear_pattern (1-D) ───────────────────────────────────────────────────────

/// DISJOINT 1-D row: count 3, spacing 2 m along +X ⇒ boxes @0,2,4 ⇒ watertight,
/// volume 3.0 m³, surface area 18.0 m².
#[test]
fn linear_pattern_1d_disjoint_volume_area_watertight() {
    let mut kernel = OcctKernel::new();
    let b = unit_box(&mut kernel);

    let row = kernel
        .execute(&GeometryOp::LinearPattern {
            target: b,
            direction: [1.0, 0.0, 0.0],
            count: 3,
            spacing: Value::Real(2.0),
        })
        .expect("disjoint linear_pattern must succeed")
        .id;

    let vol = volume_of(&mut kernel, row);
    assert!(
        within_rel(vol, 3.0, REL_TOL),
        "a disjoint 3-instance row must have volume 3.0 m³, got {vol:.9}"
    );
    let area = surface_area_of(&mut kernel, row);
    assert!(
        within_rel(area, 18.0, REL_TOL),
        "3 disjoint unit boxes must have surface area 18.0 m² (nothing merged), got {area:.9}"
    );
    assert!(
        is_watertight(&kernel, row),
        "the disjoint 1-D row result must be watertight"
    );
}

/// OVERLAPPING 1-D row: count 2, spacing 0.5 m along +X ⇒ box [-0.5,0.5] ∪
/// [0,1] spans [-0.5,1.0] ⇒ volume 1.5 m³ exactly (a bare compound would be
/// 2.0), watertight, single connected component.
#[test]
fn linear_pattern_1d_overlapping_merges_single_component() {
    let mut kernel = OcctKernel::new();
    let b = unit_box(&mut kernel);

    let row = kernel
        .execute(&GeometryOp::LinearPattern {
            target: b,
            direction: [1.0, 0.0, 0.0],
            count: 2,
            spacing: Value::Real(0.5),
        })
        .expect("overlapping linear_pattern must succeed")
        .id;

    let vol = volume_of(&mut kernel, row);
    assert!(
        within_rel(vol, 1.5, REL_TOL),
        "overlapping 2-instance row must fuse to volume 1.5 m³ (union, not 2.0 compound), got {vol:.9}"
    );
    assert!(
        is_watertight(&kernel, row),
        "the overlapping 1-D row result must be watertight"
    );
    assert!(
        is_connected(&kernel, row),
        "overlapping instances must fuse into ONE connected component"
    );
}
