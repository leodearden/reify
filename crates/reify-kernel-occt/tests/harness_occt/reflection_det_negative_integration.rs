//! A-ε probe: OCCT `det<0` output-orientation consistency, both reflection
//! paths.
//!
//! **1. Provenance.** PRD `docs/prds/v0_6/assembly-derivation-toolbox.md`
//! leaf A-ε (task #6619), boundary test T17. Answers §4's open kernel
//! question: whether OCCT 7.8's `det<0` `BRepBuilderAPI_GTransform` output is
//! consistently oriented.
//!
//! **2. OCCT version this module reasons from.** System OCCT **7.8.1**
//! (`OCC_VERSION_COMPLETE` in `/usr/include/opencascade/Standard_Version.hxx`;
//! `libTKBRep.so.7.8.1`). Per this repo's native-deps invariant, reify links
//! system 7.8 DIRECTLY — the `/opt/reify-deps` 7.9 tree is a gmsh-transitive
//! dependency only — so 7.8 is the correct reference. There is no OCCT
//! version pin anywhere in this workspace, so a future distro upgrade can
//! move this behaviour; that is precisely what makes this module a
//! regression guard rather than a one-time answer.
//!
//! **3. The answer.** `det<0` output IS consistently oriented, on BOTH
//! reflection paths. [`GeometryOp::Mirror`] (`gp_Trsf::SetMirror` →
//! `BRepBuilderAPI_Transform`) and [`GeometryOp::AffineApply`] with
//! `linear = diag(-1,1,1)` (`gp_GTrsf` → `BRepBuilderAPI_GTransform`) both
//! yield solids that are BRepCheck-valid, closed, manifold, orientable,
//! positive-volume (test 1), tessellate to an outward-wound closed orientable
//! manifold whose supplied normals agree with that winding (test 2), and
//! export to STEP as a baked `MANIFOLD_SOLID_BREP` carrying zero
//! `CARTESIAN_TRANSFORMATION_OPERATOR` entities (test 3). The mechanism: the
//! reflection SWAPS each face's orientation flag (measured 3 FORWARD / 5
//! REVERSED on a box source → 5 FORWARD / 3 REVERSED on its reflection),
//! which is exactly what keeps normals outward. **The FreeCAD
//! mirrored-bodies-vanish defect does not reproduce** — `MANIFOLD_SOLID_BREP`
//! count is >=1 in every reflected export measured.
//!
//! **4. Why the outward-winding check is load-bearing, not `mesh.validate`
//! alone.** `Mesh::validate`'s Closed + ConsistentWinding obligations are a
//! directed-edge invariant on the position-welded quotient topology, which a
//! CONSISTENTLY INWARD mesh satisfies exactly as well as an outward one. If
//! OCCT ever stopped reversing face orientation flags under a `det<0`
//! transform, every triangle of the reflected solid would flip to inward
//! winding and `validate()` would still pass silently — exactly the defect
//! T17 exists to catch. Only the per-triangle dot product of the winding
//! normal against the AABB-centre reference direction
//! ([`assert_outward_wound_closed_manifold`]) actually observes it.
//!
//! **5. Two real GTransform hazards, orthogonal to determinant sign** (test
//! 4). `BRepBuilderAPI_GTransform` rewrites every analytic surface as a
//! B-spline approximation: measured relative volume error vs source is
//! +8.615e-3 (cylinder), +8.250e-3 (cone), +1.553e-3 (torus), -4.373e-4
//! (sphere), 0 (box), and STEP loses analytic entity types (e.g. a cylinder's
//! `CYLINDRICAL_SURFACE`/`PLANE(` counts drop to 0, replaced by
//! `B_SPLINE_SURFACE`). Separately, `BRepTools_GTrsfModification` carries the
//! source's `Poly_Triangulation` across a `GTransform` UNCHANGED: if the
//! source was tessellated before the transform, that stale triangulation no
//! longer matches the rewritten B-spline geometry and
//! `BRepCheck_Analyzer::IsValid()` (`GeometryQuery::IsWatertight`) reports
//! `false` (test 4 half (b), a pinned characterization of a known defect).
//! Both hazards — the exactness loss (half (a)) and the pre-tessellation
//! invalidity (half (b)) — are tracked by follow-up task **#6652** (filed as
//! ticket `tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC`, which the curator resolved to
//! that task number); every pinning assertion in test 4 names #6652 and
//! states what its own failure would mean. BOTH hazards reproduce
//! identically under the IDENTITY linear map `diag(1,1,1)` (det = +1,
//! geometrically a no-op), which is the proof that they belong to
//! `BRepBuilderAPI_GTransform` itself and are NOT a `det<0` orientation
//! defect. [`GeometryOp::Mirror`] is immune to both: bit-exact volume,
//! preserves analytic surface types, and stays `IsWatertight` regardless of
//! tessellation ordering.
//!
//! **6. Conclusion for A-δ (#6618).** Lower reflective derivation via
//! [`GeometryOp::Mirror`] (`SetMirror`), never `AffineApply` with a `det<0`
//! linear map — `Mirror` is bit-exact, preserves analytic surface types, and
//! is immune to the pre-tessellation hazard. This corroborates PRD §3.7's
//! already-chosen v1 lowering with measured evidence rather than assumption.
//!
//! **7. Fixture contract.** Every fixture in this module is a primitive-
//! derived convex solid positioned wholly at x>0, never a boolean result — a
//! boolean op returns a `COMPOUND`, and `IsWatertight` hard-returns `false`
//! for any non-SOLID/COMPSOLID/SHELL shape regardless of actual validity
//! (see [`convex_fixtures`] for the full fixture-contract doc).

#![cfg(has_occt)]

use crate::common;
use reify_ir::{ExportFormat, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};

// ---------------------------------------------------------------------------
// Test 1 — det<0 reflection yields a valid, positive-volume solid
// ---------------------------------------------------------------------------

/// For each of three convex, primitive-derived fixtures, reflect across the
/// x=0 plane by BOTH lowerings — [`GeometryOp::Mirror`] (`gp_Trsf::SetMirror`)
/// and [`GeometryOp::AffineApply`] with `linear = diag(-1,1,1)`
/// (`gp_GTrsf` / `BRepBuilderAPI_GTransform`) — and assert that both paths
/// yield a BRepCheck-valid, positive-volume solid whose volume matches the
/// source within a path-specific tolerance.
///
/// Fixtures are deliberately primitive-derived (never a boolean result: a
/// boolean returns a COMPOUND, and `IsWatertight` hard-returns `false` for
/// any non-SOLID/COMPSOLID/SHELL shape regardless of validity) and are NOT
/// tessellated before reflecting here — pre-tessellation ordering is
/// [`gtransform_path_is_lossy_and_pretessellation_fragile_unlike_setmirror`]'s
/// subject, and tessellating in this test would contaminate its answer.
///
/// Tolerances: `Mirror` is bit-exact against the source (measured identical
/// to the last printed digit, e.g. cylinder 2.261946710585e-6 m³ both
/// sides), so 1e-12 relative is used. `AffineApply` det<0 rewrites every
/// analytic surface as a B-spline approximation; measured worst-case drift
/// is +8.615e-3 relative (cylinder) and +8.250e-3 (cone), so 2e-2 relative
/// (≈2.3× the measured worst case) is used — tightening this to, say, 1e-9
/// would be a doomed assertion.
///
/// (f) The reflected AABB (`GeometryQuery::BoundingBox` via
/// [`common::bbox_of`]) is the x-mirror of the source's:
/// `min.x == -source.max.x`, `max.x == -source.min.x`, y/z extents
/// unchanged. This is the positional obligation (a)-(e) miss: every one of
/// them is invariant under an arbitrary rigid motion, so a `Mirror` or
/// `AffineApply` that silently degraded to a translation or a no-op copy
/// would satisfy all of (a)-(e) unchanged. Tolerance is `Mirror` 1e-12
/// relative (measured bit-exact, same as the volume check) and `AffineApply`
/// 0.3 relative (**not** the volume check's 2e-2 — a separate, coarser
/// effect: `BoundingBox` computes via `BRepBndLib`'s default non-precise
/// mode, which bounds a B-spline surface by its control polygon rather than
/// the surface itself, and a NURBS approximation of a curved surface has a
/// control polygon that measurably bulges outside the true envelope;
/// measured worst case 2.105e-1 relative, on the cone). Both are scaled by
/// each fixture's own characteristic size (the largest-magnitude source AABB
/// component) rather than applied per-coordinate — a coordinate close to the
/// x=0 mirror plane would make a coordinate-relative tolerance meaningless.
#[test]
fn both_reflection_paths_yield_valid_positive_volume_solids() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    for (name, source) in convex_fixtures(&mut kernel) {
        let source_volume = volume_of(&kernel, source);
        assert!(
            source_volume > 0.0,
            "{name}: source fixture should itself have positive volume, got {source_volume:e}"
        );
        let source_bbox = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(source)));
        // Characteristic size used to scale the (f) AABB tolerance below: the
        // largest-magnitude component of the source AABB. Avoids a
        // per-coordinate relative tolerance, which would be meaningless for
        // a coordinate close to the x=0 mirror plane.
        let bbox_scale = [
            source_bbox.xmin,
            source_bbox.xmax,
            source_bbox.ymin,
            source_bbox.ymax,
            source_bbox.zmin,
            source_bbox.zmax,
        ]
        .into_iter()
        .fold(0.0_f64, |acc, v| acc.max(v.abs()));

        let mirrored = mirror_across_yz(&mut kernel, source);
        let affine = affine_reflect_x(&mut kernel, source);

        // (path, target, volume-relative-tolerance, bbox-relative-tolerance).
        // The two tolerances are DELIBERATELY different bases, not a shared
        // `tol` — see the (f) comment below for why a shared tolerance would
        // be doomed.
        for (path, target, vol_tol, bbox_tol) in [
            ("Mirror", mirrored, 1e-12, 1e-12),
            ("AffineApply", affine, 2e-2, 0.3),
        ] {
            // (b) positive volume under reflection.
            let v = volume_of(&kernel, target);
            assert!(
                v > 0.0,
                "{name} via {path}: reflected volume must be positive, got {v:e}"
            );

            // (c)/(d) volume matches source within the path-specific tolerance.
            let rel_err = (v - source_volume).abs() / source_volume;
            assert!(
                rel_err < vol_tol,
                "{name} via {path}: reflected volume {v:e} should match source {source_volume:e} \
                 within {vol_tol:e} relative, got rel_err={rel_err:e}"
            );

            // (e) BRepCheck validity survives reflection.
            for (flag_name, query) in [
                ("IsWatertight", GeometryQuery::IsWatertight(target)),
                ("IsManifold", GeometryQuery::IsManifold(target)),
                ("IsOrientable", GeometryQuery::IsOrientable(target)),
                ("IsClosed", GeometryQuery::IsClosed(target)),
            ] {
                assert!(
                    flag_of(&kernel, query),
                    "{name} via {path}: {flag_name} should be true after det<0 reflection"
                );
            }

            // (f) the reflected AABB is the x-mirror of the source's: x
            // flips and negates, y/z are unchanged. Catches a `Mirror` or
            // `AffineApply` that silently degraded to a translation or a
            // no-op copy — invisible to (b)-(e), which are invariant under
            // any rigid motion.
            //
            // `bbox_tol` is NOT the volume tolerance reused: measured, the
            // two are different-sized effects. `Mirror` is bit-exact here
            // too (measured delta 0 on all 6 fields × all 3 fixtures), so it
            // keeps the same 1e-12 relative bound. `AffineApply`'s bbox error
            // is a SEPARATE, much coarser phenomenon than its ~8.6e-3 volume
            // error: `GeometryQuery::BoundingBox` computes via `BRepBndLib`'s
            // default (non-precise) mode, which bounds a B-spline surface by
            // its CONTROL POLYGON rather than the surface itself — and a
            // NURBS approximation of a circular/conical surface has a
            // control polygon that measurably bulges outside the true
            // envelope. Measured worst case 2.105e-1 relative to
            // `bbox_scale` (cone xmax); box is unaffected (delta 0 — a
            // B-spline image of a flat plane is exact, matching test 4's
            // finding). 0.3 (~1.4× the measured worst case) is used —
            // reusing the 2e-2 volume band here would be a doomed assertion.
            let target_bbox = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(target)));
            let bbox_abs_tol = bbox_tol * bbox_scale;
            for (field, actual, expected) in [
                ("xmin", target_bbox.xmin, -source_bbox.xmax),
                ("xmax", target_bbox.xmax, -source_bbox.xmin),
                ("ymin", target_bbox.ymin, source_bbox.ymin),
                ("ymax", target_bbox.ymax, source_bbox.ymax),
                ("zmin", target_bbox.zmin, source_bbox.zmin),
                ("zmax", target_bbox.zmax, source_bbox.zmax),
            ] {
                let delta = (actual - expected).abs();
                assert!(
                    delta < bbox_abs_tol,
                    "{name} via {path}: reflected AABB.{field} should equal {expected:e} \
                     (x-mirror of the source AABB across x=0; y/z unchanged), got {actual:e} \
                     (delta {delta:e}, tol {bbox_abs_tol:e} = {bbox_tol:e} rel × characteristic \
                     scale {bbox_scale:e})"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers — kept PRIVATE to this module: they encode this probe's
// fixture contract, not a crate-wide idiom. `tests/common/mod.rs` is
// reserved for helpers duplicated across MANY modules (see its header).
// ---------------------------------------------------------------------------

/// Build the three convex, primitive-derived fixtures this module's tests
/// share: a box, a cylinder and a cone, each translated wholly into x>0.
///
/// Two constraints future editors must not break:
///   - **Primitive-derived only, never a boolean result.** `BRepAlgoAPI_Cut`
///     (and Union/Intersection) return a `COMPOUND`, and `IsWatertight`
///     (`occt_wrapper.cpp`) hard-returns `false` for any shape that is not
///     SOLID/COMPSOLID/SHELL — regardless of actual validity. A boolean-
///     derived fixture would make this module's validity assertions
///     unsatisfiable.
///   - **Positioned wholly at x>0.**
///     [`reflected_brep_step_export_bakes_geometry_and_emits_no_det_negative_placement`]'s
///     baked-geometry STEP assertion reads a negative x coordinate in an
///     exported 3D `CARTESIAN_POINT` entity as the reflection signal; a
///     fixture straddling or left of x=0 would make that signal ambiguous.
///     (The signal is counted by `negative_x_3d_point_count`, which excludes
///     2D pcurve parameter-space points — the cone source emits 3 of those
///     and they are NOT positions.)
///
/// All three are additionally CONVEX, which is what licenses the AABB-centre
/// reference direction in the outward-winding tessellation check
/// ([`both_reflection_paths_tessellate_to_outward_wound_closed_manifold`])
/// — a concave fixture can legitimately have inward-pointing dot products in
/// its concave regions (measured: 343 outward / 253 inward on a box-minus-
/// cylinder-minus-sphere part), which would make that assertion meaningless.
// Fixture dimensions — single source of truth shared by `convex_fixtures` and
// `fresh_pretessellated_cylinder`, so the two can never silently drift apart:
// `fresh_pretessellated_cylinder` must build the exact same `cylinder_r6_h20`
// that `convex_fixtures` does.
const BOX_WIDTH: f64 = 0.010;
const BOX_HEIGHT: f64 = 0.020;
const BOX_DEPTH: f64 = 0.030;
const BOX_DX: f64 = 0.050;

const CYLINDER_RADIUS: f64 = 0.006;
const CYLINDER_HEIGHT: f64 = 0.020;
const CYLINDER_DX: f64 = 0.030;
const CYLINDER_DY: f64 = 0.004;

const CONE_BOTTOM_RADIUS: f64 = 0.008;
const CONE_TOP_RADIUS: f64 = 0.004;
const CONE_HEIGHT: f64 = 0.015;
const CONE_DX: f64 = 0.030;

fn convex_fixtures(kernel: &mut OcctKernel) -> Vec<(&'static str, GeometryHandleId)> {
    let box_src = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(BOX_WIDTH),
            height: Value::Real(BOX_HEIGHT),
            depth: Value::Real(BOX_DEPTH),
        })
        .expect("box_10x20x30 should build");
    let box_id = kernel
        .execute(&GeometryOp::Translate {
            target: box_src.id,
            dx: BOX_DX,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_10x20x30 translate to x>0 should succeed")
        .id;

    let cyl_src = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(CYLINDER_RADIUS),
            height: Value::Real(CYLINDER_HEIGHT),
        })
        .expect("cylinder_r6_h20 should build");
    let cyl_id = kernel
        .execute(&GeometryOp::Translate {
            target: cyl_src.id,
            dx: CYLINDER_DX,
            dy: CYLINDER_DY,
            dz: 0.0,
        })
        .expect("cylinder_r6_h20 translate to x>0 should succeed")
        .id;

    let cone_src = kernel
        .execute(&GeometryOp::Cone {
            bottom_radius: Value::Real(CONE_BOTTOM_RADIUS),
            top_radius: Value::Real(CONE_TOP_RADIUS),
            height: Value::Real(CONE_HEIGHT),
        })
        .expect("cone_r8_r4_h15 should build");
    let cone_id = kernel
        .execute(&GeometryOp::Translate {
            target: cone_src.id,
            dx: CONE_DX,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("cone_r8_r4_h15 translate to x>0 should succeed")
        .id;

    vec![
        ("box_10x20x30", box_id),
        ("cylinder_r6_h20", cyl_id),
        ("cone_r8_r4_h15", cone_id),
    ]
}

/// Mirror `target` across the x=0 (y-z) plane via [`GeometryOp::Mirror`]
/// (`gp_Trsf::SetMirror`) — the v1 reflective-derivation lowering PRD §3.7
/// names, and the path this module's probe finds bit-exact and immune to
/// both GTransform hazards
/// ([`gtransform_path_is_lossy_and_pretessellation_fragile_unlike_setmirror`]).
fn mirror_across_yz(kernel: &mut OcctKernel, target: GeometryHandleId) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Mirror {
            target,
            plane_origin: [0.0, 0.0, 0.0],
            plane_normal: [1.0, 0.0, 0.0],
        })
        .expect("Mirror across the x=0 plane should succeed for a det<0 reflection")
        .id
}

/// Apply the general dense 3×3 linear map `linear` (zero translation) to
/// `target` via [`GeometryOp::AffineApply`] (`gp_GTrsf` /
/// `BRepBuilderAPI_GTransform`). [`affine_reflect_x`] delegates here with
/// `diag(-1,1,1)`;
/// [`gtransform_path_is_lossy_and_pretessellation_fragile_unlike_setmirror`]
/// reuses this general form directly with the IDENTITY `diag(1,1,1)` to
/// prove its two GTransform hazards are determinant-independent rather than
/// reflection artifacts.
fn affine_linear(
    kernel: &mut OcctKernel,
    target: GeometryHandleId,
    linear: [[f64; 3]; 3],
) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::AffineApply {
            target,
            linear,
            translation: [0.0, 0.0, 0.0],
        })
        .unwrap_or_else(|e| {
            panic!(
                "AffineApply({linear:?}) on handle {target:?} should succeed: neither the Rust \
                 finiteness guard nor the C++ Hadamard singularity guard should reject a \
                 det<0 (or det=1 identity) linear map, got {e:?}"
            )
        })
        .id
}

/// Reflect `target` across the x=0 (y-z) plane via [`GeometryOp::AffineApply`]
/// with `linear = diag(-1,1,1)` (det = -1) — the general `gp_GTrsf` /
/// `BRepBuilderAPI_GTransform` path, contrasted against [`mirror_across_yz`]'s
/// dedicated `gp_Trsf::SetMirror` path.
fn affine_reflect_x(kernel: &mut OcctKernel, target: GeometryHandleId) -> GeometryHandleId {
    affine_linear(
        kernel,
        target,
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    )
}

/// Query the volume of `id` in m³, panicking (naming the received `Value`
/// variant) if the kernel returns anything other than a numeric value.
///
/// Mirrors the strict `Value`-unwrapping convention of `tests/common/mod.rs`
/// (`parse_bbox`/`xyz_of`): a mismatched shape panics loudly rather than
/// silently defaulting, so a malformed kernel response surfaces as a parse
/// failure rather than a confusing downstream geometry assertion.
fn volume_of(kernel: &OcctKernel, id: GeometryHandleId) -> f64 {
    let value = kernel
        .query(&GeometryQuery::Volume(id))
        .unwrap_or_else(|e| panic!("Volume query on handle {id:?} should succeed: {e:?}"));
    value.as_f64().unwrap_or_else(|| {
        panic!("Volume query on handle {id:?} should be numeric, got {value:?}")
    })
}

/// Evaluate a boolean [`GeometryQuery`] (e.g. `IsWatertight`, `IsManifold`),
/// panicking (naming the received `Value` variant) if the kernel returns
/// anything other than `Value::Bool`. Mirrors [`volume_of`]'s strictness
/// convention.
fn flag_of(kernel: &OcctKernel, query: GeometryQuery) -> bool {
    let value = kernel
        .query(&query)
        .unwrap_or_else(|e| panic!("{query:?} should succeed: {e:?}"));
    match value {
        Value::Bool(b) => b,
        other => panic!("{query:?} should return Value::Bool, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test 2 — det<0 reflection tessellates to an outward-wound, closed,
// orientable manifold. THE CORE of T17: this is the assertion that actually
// observes det<0 output orientation.
// ---------------------------------------------------------------------------

/// For each convex fixture and each of the two reflection paths, tessellate
/// at 1e-4 m (0.1 mm) deflection and assert the result is a closed,
/// consistently OUTWARD-wound manifold whose supplied normals agree with
/// that winding.
///
/// **Why the outward-winding check (obligation (b) on
/// [`assert_outward_wound_closed_manifold`]) is load-bearing, and
/// `mesh.validate(0.0)` (obligation (a)) is NOT sufficient on its own:**
/// `Mesh::validate`'s Closed + ConsistentWinding obligations are a directed-
/// edge invariant on the position-welded quotient topology, which a
/// CONSISTENTLY INWARD mesh satisfies exactly as well as an outward one. If
/// OCCT ever stopped reversing face orientation flags under a det<0
/// transform (measured today: a 3 FORWARD/5 REVERSED box flips to 5
/// FORWARD/3 REVERSED under reflection), every triangle of the reflected
/// solid would flip to inward winding and `validate()` would still pass —
/// exactly the defect T17 exists to catch would slip through silently. Only
/// the per-triangle dot product against the AABB-centre reference direction
/// actually observes it, which is exactly what would fail loudly if that
/// regression ever happened.
///
/// **Convexity precondition**: all three fixtures are convex (documented on
/// [`convex_fixtures`]), which is what licenses using the AABB centre as the
/// "outward" reference direction. A non-convex fixture can legitimately
/// produce inward-pointing dot products in its concave regions — measured
/// 343 outward / 253 inward triangles on a box-minus-cylinder-minus-sphere
/// part, which is correct behaviour for a concave part, not a defect. Do NOT
/// add a non-convex fixture to this test; it would make the assertion
/// meaningless rather than stricter.
#[test]
fn both_reflection_paths_tessellate_to_outward_wound_closed_manifold() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    for (name, source) in convex_fixtures(&mut kernel) {
        let mirrored = mirror_across_yz(&mut kernel, source);
        let affine = affine_reflect_x(&mut kernel, source);

        for (path, target) in [("Mirror", mirrored), ("AffineApply", affine)] {
            let mesh = kernel.tessellate(target, 1e-4).unwrap_or_else(|e| {
                panic!("{name} via {path}: tessellate(1e-4) should succeed: {e:?}")
            });
            assert_outward_wound_closed_manifold(&mesh, &format!("{name} via {path}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Tessellation-orientation helpers
// ---------------------------------------------------------------------------

/// Compute the geometric normal of triangle (pa, pb, pc) from the emitted
/// winding order: AB × AC. All inputs and the result are in f64.
///
/// Verbatim-shaped reuse of the same-named helper in
/// `tessellation_winding_integration.rs`. Intentionally duplicated rather
/// than hoisted to `tests/common/mod.rs` (reserved for helpers duplicated
/// across MANY modules — see its header): this one is shared by exactly two
/// sibling submodules of the same compile unit, and keeping the name
/// identical documents the kinship for a reader rather than hiding it.
fn tri_winding_normal(pa: [f64; 3], pb: [f64; 3], pc: [f64; 3]) -> [f64; 3] {
    let ab = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let ac = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
    [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ]
}

/// Per-axis (min+max)/2 over `verts` — the AABB-centre reference direction
/// used by [`assert_outward_wound_closed_manifold`]'s outward-winding check.
/// Robust to non-uniform vertex density across faces, unlike a vertex-cloud
/// mean (same rationale as `tessellation_winding_integration.rs`'s
/// `box_centroid`).
fn aabb_centre(verts: &[[f32; 3]]) -> [f64; 3] {
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    for v in verts {
        for k in 0..3 {
            let coord = v[k] as f64;
            if coord < min[k] {
                min[k] = coord;
            }
            if coord > max[k] {
                max[k] = coord;
            }
        }
    }
    [
        (min[0] + max[0]) / 2.0,
        (min[1] + max[1]) / 2.0,
        (min[2] + max[2]) / 2.0,
    ]
}

/// Assert that `mesh` is a closed, orientable manifold whose triangles are
/// ALL outward-wound and whose supplied per-vertex normals agree with that
/// winding — the full T17 core obligation ([`both_reflection_paths_tessellate_to_outward_wound_closed_manifold`]'s
/// doc comment explains why obligation (b) below is the load-bearing one and
/// (a) alone is not sufficient).
///
/// `what` is interpolated into every panic message (e.g.
/// `"cylinder_r6_h20 via AffineApply"`) so a failure identifies which
/// fixture × path regressed. `#[track_caller]` so a failure points at the
/// calling test line, matching this crate's `assert_aabb_eq` /
/// `assert_records_in_range` convention.
///
/// Obligations, verbatim-shaped from `tessellation_winding_integration.rs`'s
/// two tests (this module's convex reflected fixtures are the subject; that
/// module's plain OCCT box is the precedent this reuses):
///
///   (a) `mesh.validate(0.0)` is `Ok` — the INV-GEO-1 mesh contract (Closed +
///       ConsistentWinding + NonDegenerate on the position-welded quotient
///       topology). `tol = 0.0`: this asserts real OCCT tessellation output,
///       not a distance-tolerant approximation.
///   (b) EVERY triangle is OUTWARD-wound: over the welded canonical
///       positions, the winding normal `AB × AC` must have a strictly
///       positive dot product with (triangle centroid − AABB centre).
///   (c) EVERY triangle's averaged supplied per-vertex normal (over the RAW
///       unwelded indices/vertices) agrees (dot > 0) with its raw winding
///       normal.
#[track_caller]
fn assert_outward_wound_closed_manifold(mesh: &reify_ir::Mesh, what: &str) {
    // (a) INV-GEO-1 mesh contract.
    mesh.validate(0.0).unwrap_or_else(|e| {
        panic!("{what}: mesh.validate(0.0) should succeed (INV-GEO-1 contract), got {e:?}")
    });

    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "{what}: index count must be a multiple of 3"
    );
    let num_tris = mesh.indices.len() / 3;

    // (b) Outward winding over the welded canonical positions.
    let (canon_verts, welded) = mesh.weld_positions();
    let centre = aabb_centre(&canon_verts);

    for t in 0..num_tris {
        let ia = welded[mesh.indices[t * 3] as usize] as usize;
        let ib = welded[mesh.indices[t * 3 + 1] as usize] as usize;
        let ic = welded[mesh.indices[t * 3 + 2] as usize] as usize;
        let pa = canon_verts[ia];
        let pb = canon_verts[ib];
        let pc = canon_verts[ic];

        let pa_f64 = [pa[0] as f64, pa[1] as f64, pa[2] as f64];
        let pb_f64 = [pb[0] as f64, pb[1] as f64, pb[2] as f64];
        let pc_f64 = [pc[0] as f64, pc[1] as f64, pc[2] as f64];
        let normal = tri_winding_normal(pa_f64, pb_f64, pc_f64);

        let tri_centroid = [
            (pa_f64[0] + pb_f64[0] + pc_f64[0]) / 3.0,
            (pa_f64[1] + pb_f64[1] + pc_f64[1]) / 3.0,
            (pa_f64[2] + pb_f64[2] + pc_f64[2]) / 3.0,
        ];
        let outward = [
            tri_centroid[0] - centre[0],
            tri_centroid[1] - centre[1],
            tri_centroid[2] - centre[2],
        ];
        let dot = normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2];

        assert!(
            dot > 0.0,
            "{what}: triangle {t} (welded verts {ia},{ib},{ic}) geometric normal from emitted \
             winding points inward (dot = {dot:.6}); every triangle must be outward-wound"
        );
    }

    // (c) Supplied normals agree with the RAW (unwelded) winding.
    let supplied = mesh
        .normals
        .as_ref()
        .unwrap_or_else(|| panic!("{what}: tessellate should emit per-vertex normals"));
    assert_eq!(
        supplied.len(),
        mesh.vertices.len(),
        "{what}: normals array must have same length as vertices array"
    );

    for t in 0..num_tris {
        let i0 = mesh.indices[t * 3] as usize;
        let i1 = mesh.indices[t * 3 + 1] as usize;
        let i2 = mesh.indices[t * 3 + 2] as usize;

        let pa = [
            mesh.vertices[i0 * 3] as f64,
            mesh.vertices[i0 * 3 + 1] as f64,
            mesh.vertices[i0 * 3 + 2] as f64,
        ];
        let pb = [
            mesh.vertices[i1 * 3] as f64,
            mesh.vertices[i1 * 3 + 1] as f64,
            mesh.vertices[i1 * 3 + 2] as f64,
        ];
        let pc = [
            mesh.vertices[i2 * 3] as f64,
            mesh.vertices[i2 * 3 + 1] as f64,
            mesh.vertices[i2 * 3 + 2] as f64,
        ];
        let winding_normal = tri_winding_normal(pa, pb, pc);

        let avg_supplied = [
            (supplied[i0 * 3] as f64 + supplied[i1 * 3] as f64 + supplied[i2 * 3] as f64) / 3.0,
            (supplied[i0 * 3 + 1] as f64
                + supplied[i1 * 3 + 1] as f64
                + supplied[i2 * 3 + 1] as f64)
                / 3.0,
            (supplied[i0 * 3 + 2] as f64
                + supplied[i1 * 3 + 2] as f64
                + supplied[i2 * 3 + 2] as f64)
                / 3.0,
        ];

        let dot = winding_normal[0] * avg_supplied[0]
            + winding_normal[1] * avg_supplied[1]
            + winding_normal[2] * avg_supplied[2];

        assert!(
            dot > 0.0,
            "{what}: triangle {t} (raw verts {i0},{i1},{i2}) supplied normals (avg \
             [{:.4},{:.4},{:.4}]) disagree with the geometric winding normal (dot = {dot:.6}); \
             supplied normals must agree with the outward-wound triangles",
            avg_supplied[0],
            avg_supplied[1],
            avg_supplied[2],
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3 — reflected B-rep STEP export: baked geometry, no det<0 placement.
// ---------------------------------------------------------------------------

/// The STEP-writer half of T17: for each convex fixture, export the SOURCE
/// and BOTH reflections and assert the writer emits a valid det=+1 assembly
/// from a reflected B-rep, with the impropriety BAKED into geometry rather
/// than placed (PRD §3.8: "baked reflected B-reps + det=+1 placements are
/// AP242-conformant by construction"). The FreeCAD mirrored-bodies-vanish
/// defect is the cautionary tale this test rules out.
///
/// `src/lib.rs`'s existing `new_ops_export_step` unit test already exports a
/// mirrored box, but asserts only that the file contains `ISO-10303-21` —
/// cited here as the shallow precedent this module deepens, not duplicated.
///
/// The workspace has NO STEP reader anywhere (no `STEPControl_Reader` /
/// `STEPCAFControl_Reader`), so textual assertion over the exported content
/// is the only verification available — and, per the five obligations below,
/// sufficient.
#[test]
fn reflected_brep_step_export_bakes_geometry_and_emits_no_det_negative_placement() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    for (name, source) in convex_fixtures(&mut kernel) {
        let mirrored = mirror_across_yz(&mut kernel, source);
        let affine = affine_reflect_x(&mut kernel, source);

        let source_text = step_text(&kernel, source);

        // (a) well-formed STEP framing on the source export.
        assert!(
            source_text.contains("ISO-10303-21") && source_text.contains("END-ISO-10303-21"),
            "{name} source: STEP export should contain ISO-10303-21/END-ISO-10303-21 framing"
        );
        // Baseline for (c): the source's own ADVANCED_FACE count.
        let source_faces = step_entity_count(&source_text, "ADVANCED_FACE");
        // (e) baked-geometry negative control: the source lives wholly at x>0,
        // so it must carry no negative-x 3D CARTESIAN_POINT entity. Counted
        // via `negative_x_3d_point_count`, NOT a raw
        // `CARTESIAN_POINT('',(-` substring: see that helper's doc — the raw
        // literal also matches 2D pcurve parameter-space points, of which the
        // cone source legitimately exports 3.
        assert_eq!(
            negative_x_3d_point_count(&source_text),
            0,
            "{name} source: wholly-x>0 fixture should export NO 3D CARTESIAN_POINT \
             entity with a negative x coordinate"
        );
        // (d) det=+1 by construction on the source too.
        assert_eq!(
            step_entity_count(&source_text, "CARTESIAN_TRANSFORMATION_OPERATOR"),
            0,
            "{name} source: writer should emit zero CARTESIAN_TRANSFORMATION_OPERATOR \
             entities — every placement is an AXIS2_PLACEMENT_3D (y derived as z×x, \
             right-handed by construction), so counting zero is equivalent to det=+1"
        );

        for (path, target) in [("Mirror", mirrored), ("AffineApply", affine)] {
            let text = step_text(&kernel, target);

            // (a) well-formed STEP framing; a truncated/failed write is not
            // silently accepted.
            assert!(
                text.contains("ISO-10303-21") && text.contains("END-ISO-10303-21"),
                "{name} via {path}: STEP export should contain ISO-10303-21/END-ISO-10303-21 \
                 framing"
            );

            // (b) the mirrored solid did NOT vanish (the FreeCAD cautionary tale).
            assert!(
                step_entity_count(&text, "MANIFOLD_SOLID_BREP") >= 1,
                "{name} via {path}: reflected export should contain >=1 MANIFOLD_SOLID_BREP \
                 — the mirrored solid must not vanish"
            );

            // (c) no face dropped by the reflection.
            assert_eq!(
                step_entity_count(&text, "ADVANCED_FACE"),
                source_faces,
                "{name} via {path}: reflected ADVANCED_FACE count should equal the source's \
                 ({source_faces}) — no face should be dropped by the reflection"
            );

            // (d) det=+1 assertion: the writer emits no transformation operator at
            // all, so every placement is an AXIS2_PLACEMENT_3D, whose y-axis is
            // DERIVED as z×x and therefore cannot encode a left-handed frame —
            // det=+1 holds by construction, not by numeric check. Substring-match
            // (not an exact-token match) so the `_3D`-suffixed spelling is covered.
            assert_eq!(
                step_entity_count(&text, "CARTESIAN_TRANSFORMATION_OPERATOR"),
                0,
                "{name} via {path}: writer should emit zero CARTESIAN_TRANSFORMATION_OPERATOR \
                 entities — every placement is an AXIS2_PLACEMENT_3D (y derived as z×x, \
                 right-handed by construction and therefore unable to encode a left-handed \
                 frame), so counting zero is equivalent to asserting every placement has \
                 det=+1, with no float comparison"
            );

            // (e) geometry is BAKED, not placed: the reflected export carries >=1
            // negative-x 3D CARTESIAN_POINT while the wholly-x>0 source carries
            // exactly 0 — so a negative x coordinate can only come from baked
            // mirrored geometry.
            assert!(
                negative_x_3d_point_count(&text) >= 1,
                "{name} via {path}: reflected export should contain >=1 3D CARTESIAN_POINT \
                 with a negative x coordinate (baked mirrored geometry)"
            );
            // (e, cont.) negative control: a FULLY reflected fixture should carry
            // ZERO positive-x 3D CARTESIAN_POINTs. A nonzero count would mean some
            // geometry was not reflected — a partial reflection (some faces
            // reflected, some not) or a reflection about the wrong plane — which
            // the >=1 negative-x check above cannot rule out on its own.
            assert_eq!(
                positive_x_3d_point_count(&text),
                0,
                "{name} via {path}: fully-reflected export should contain NO 3D \
                 CARTESIAN_POINT with a positive x coordinate — a nonzero count would mean \
                 some geometry was not reflected (partial reflection, or reflection about \
                 the wrong plane)"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// STEP text helpers
// ---------------------------------------------------------------------------

/// Export `id` to STEP text via an in-memory buffer:
/// `kernel.export(id, ExportFormat::Step, &mut buf)` then
/// `String::from_utf8(buf)`, both unwrapped with a message naming the
/// handle. Reused by
/// [`gtransform_path_is_lossy_and_pretessellation_fragile_unlike_setmirror`]'s
/// cross-path entity-count comparison.
fn step_text(kernel: &OcctKernel, id: GeometryHandleId) -> String {
    let mut buf = Vec::<u8>::new();
    kernel
        .export(id, ExportFormat::Step, &mut buf)
        .unwrap_or_else(|e| panic!("STEP export of handle {id:?} should succeed: {e:?}"));
    String::from_utf8(buf)
        .unwrap_or_else(|e| panic!("STEP export of handle {id:?} should be valid UTF-8: {e}"))
}

/// Count non-overlapping occurrences of `entity` in `step_text` via a plain
/// whole-string [`str::match_indices`] scan.
///
/// **This is fold-fragile, not fold-safe.** STEP's line-folding inserts a
/// newline into the byte stream, so a token that folds mid-entity (e.g.
/// inside `PLANE(` or `CARTESIAN_TRANSFORMATION_OPERATOR`) is missed by this
/// whole-string scan exactly as it would be by a line-based one — the
/// newline is still there, splitting the match either way; scanning the
/// whole string only helps for a match that spans a line boundary with no
/// inserted character, which folding never produces. This module's three
/// fixtures export small STEP files (118-149 entities measured), well under
/// any line-folding threshold, so nothing folds today and every count below
/// is exact. A future fixture large enough to fold would need either a
/// normalize-then-count pass (strip inserted line breaks before scanning) or
/// a documented re-verification that folding still doesn't reach the counted
/// tokens.
///
/// This is a substring match, not an exact-token match, so a suffixed
/// spelling (e.g. `CARTESIAN_TRANSFORMATION_OPERATOR_3D`) is still counted.
/// Two tokens used in this module need care as a result, and are
/// nonetheless unambiguous:
///
///   - `PLANE(` must be spelled WITH the open paren — bare `PLANE` would
///     also match `PLANE_ANGLE_MEASURE_WITH_UNIT` / `PLANE_ANGLE_UNIT`.
///     Verified: `PLANE(` yields exactly 2 on a cylinder source (the two
///     planar caps), 6 on a box.
///   - `MANIFOLD_SOLID_BREP` does not collide with
///     `ADVANCED_BREP_SHAPE_REPRESENTATION` (no shared substring spans the
///     token boundary between them).
fn step_entity_count(step_text: &str, entity: &str) -> usize {
    step_text.match_indices(entity).count()
}

/// Count `CARTESIAN_POINT` entities that are **3D model-space** points whose
/// x (first) coordinate satisfies `pred` — i.e. baked geometry on one
/// particular side of the x=0 plane. Shared by [`negative_x_3d_point_count`]
/// and [`positive_x_3d_point_count`].
///
/// Deliberately NOT a plain `step_entity_count(text, "CARTESIAN_POINT('',(-")`.
/// That literal is a FALSE-POSITIVE detector, and the cone fixture proves it:
/// `cone_r8_r4_h15`'s untransformed (x>0) SOURCE export contains 3 such
/// matches, all of them **2D parameter-space** points inside pcurve
/// `DEFINITIONAL_REPRESENTATION` / `SEAM_CURVE` entries under a
/// `REPRESENTATION_CONTEXT('2D SPACE','')` — `(-0.,-15.)`, `(-0.,-0.)` and
/// `(-6.28318530718,0.)`, the last being the −2π periodic seam-parameter
/// wrap of the conical surface. Those (u,v) parameters have nothing to do
/// with 3D position, and the box and cylinder fixtures happen not to emit
/// any (measured 0 / 0 / 3 across box / cylinder / cone sources), which is
/// why the naive literal looked sound until the cone was measured.
///
/// Discriminator: a 3D `CARTESIAN_POINT` carries exactly three coordinates,
/// a parameter-space one exactly two. So parse the parenthesised coordinate
/// list and require arity 3 plus `pred(x)`. Parsing (rather than a
/// `starts_with('-')` text test) also drops the STEP writer's signed-zero
/// spelling `-0.` for free: IEEE `-0.0 < 0.0` and `-0.0 > 0.0` are both
/// `false`, so a signed zero counts toward neither
/// [`negative_x_3d_point_count`] nor [`positive_x_3d_point_count`].
///
/// Whole-string scan, same fold caveat as [`step_entity_count`] (fold-fragile
/// in principle, harmless on this module's small fixtures); the per-field
/// `trim` absorbs any fold whitespace inside the coordinate list.
fn count_3d_points_matching(step_text: &str, pred: impl Fn(f64) -> bool) -> usize {
    const HEAD: &str = "CARTESIAN_POINT('',(";
    step_text
        .match_indices(HEAD)
        .filter(|(at, _)| {
            let rest = &step_text[at + HEAD.len()..];
            let Some(end) = rest.find(')') else {
                return false;
            };
            let coords: Vec<&str> = rest[..end].split(',').map(str::trim).collect();
            coords.len() == 3 && coords[0].parse::<f64>().is_ok_and(&pred)
        })
        .count()
}

/// Count 3D `CARTESIAN_POINT` entities with a strictly negative x coordinate
/// — baked geometry left of the x=0 plane. See [`count_3d_points_matching`]
/// for the arity discriminator and why a raw substring probe is unsound.
fn negative_x_3d_point_count(step_text: &str) -> usize {
    count_3d_points_matching(step_text, |x| x < 0.0)
}

/// Count 3D `CARTESIAN_POINT` entities with a strictly positive x coordinate.
/// Used as the negative control on a reflected export: a fixture that lives
/// wholly at x>0 before reflection should, after a FULL reflection, carry
/// ZERO such points — a nonzero count means some geometry was not reflected
/// (a partial reflection, or a reflection about the wrong plane). See
/// [`count_3d_points_matching`] for the arity discriminator.
fn positive_x_3d_point_count(step_text: &str) -> usize {
    count_3d_points_matching(step_text, |x| x > 0.0)
}

// ---------------------------------------------------------------------------
// Test 4 — GTransform hazards are real but ORTHOGONAL to determinant sign:
// analytic-geometry loss and pre-tessellation fragility. This is the
// A-δ-informing payload of the probe.
// ---------------------------------------------------------------------------

/// Two `BRepBuilderAPI_GTransform` hazards that a reader could otherwise
/// mis-attribute to the det<0 orientation question tests 1-3 above answer
/// cleanly: they are real, but they are artifacts of
/// `GTransform`/`BRepTools_GTrsfModification` itself, not of reflection or
/// determinant sign. Both halves below also exercise the IDENTITY linear map
/// `diag(1,1,1)` (det = +1, geometrically a no-op) alongside the det<0
/// reflection: reproducing a hazard under the identity map is what turns
/// "OCCT mishandles det<0" from a plausible misreading of this probe into a
/// disproven one — precisely the distinction PRD §4's open question turns on.
///
/// Both hazards below are tracked by follow-up task **#6652** (filed as
/// ticket `tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC`, which the curator resolved to
/// that task number).
///
/// **Half (a) — analytic geometry is destroyed** (fixture never tessellated).
/// `BRepBuilderAPI_GTransform` rewrites every analytic surface (planes,
/// cylinders, ...) as a B-spline approximation; `GeometryOp::Mirror`
/// (`gp_Trsf::SetMirror`) does not, so analytic surface types and exact
/// volume survive it unchanged. Its four assertions pin TODAY's lossy
/// behaviour as a hard gate — **if any of them FAILS, OCCT has gotten
/// BETTER at preserving analytic geometry under GTransform: update that
/// assertion and this module's doc (and #6652), rather than treating the
/// failure as a regression.**
///
/// **Half (b) — pre-tessellation fragility**, and why it needs a CURVED
/// fixture. `BRepTools_GTrsfModification` (GTransform's modifier) rewrites
/// analytic geometry but carries the source's `Poly_Triangulation` across
/// UNCHANGED. If the source was tessellated before the transform, that stale
/// triangulation no longer matches the new B-spline geometry and
/// `BRepCheck_Analyzer::IsValid()` (`GeometryQuery::IsWatertight`) reports
/// `false`. A box fixture would NOT show this: the B-spline image of a flat
/// plane is exact, so a stale planar triangulation still matches it — only a
/// curved surface exposes the mismatch, hence the cylinder. `GeometryOp::Mirror`
/// is immune: `gp_Trsf` is a true isometry that never touches the shape's
/// underlying geometry representation, so the carried triangulation always
/// still matches.
///
/// The two `IsWatertight == false` assertions in half (b) are CHARACTERIZATION
/// PINS of this known defect (follow-up task #6652 / ticket
/// `tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC`), not desired behaviour — **if either
/// assertion FAILS, the defect has been FIXED: delete that pin, update this
/// module's doc, and close #6652.**
#[test]
fn gtransform_path_is_lossy_and_pretessellation_fragile_unlike_setmirror() {
    if !OCCT_AVAILABLE {
        return;
    }

    // --- Half (a): analytic geometry is destroyed (never-tessellated source) ---
    let mut kernel = OcctKernel::new();
    let (_, cylinder) = convex_fixtures(&mut kernel)
        .into_iter()
        .find(|(name, _)| *name == "cylinder_r6_h20")
        .expect("convex_fixtures should include cylinder_r6_h20");

    let source_text = step_text(&kernel, cylinder);
    let source_volume = volume_of(&kernel, cylinder);
    let source_cyl = step_entity_count(&source_text, "CYLINDRICAL_SURFACE");
    let source_planes = step_entity_count(&source_text, "PLANE(");
    assert_eq!(
        source_cyl, 1,
        "cylinder_r6_h20 source should export exactly 1 CYLINDRICAL_SURFACE"
    );
    assert_eq!(
        source_planes, 2,
        "cylinder_r6_h20 source should export exactly 2 PLANE( entities (the two end caps)"
    );
    assert_eq!(
        step_entity_count(&source_text, "B_SPLINE_SURFACE"),
        0,
        "cylinder_r6_h20 source should export no B_SPLINE_SURFACE entities"
    );

    let mirrored = mirror_across_yz(&mut kernel, cylinder);
    let affine = affine_reflect_x(&mut kernel, cylinder);

    // Mirror preserves analytic geometry and volume exactly.
    let mirrored_text = step_text(&kernel, mirrored);
    assert_eq!(
        step_entity_count(&mirrored_text, "CYLINDRICAL_SURFACE"),
        source_cyl,
        "Mirror should preserve the source's CYLINDRICAL_SURFACE count exactly \
         (gp_Trsf::SetMirror never rewrites analytic geometry)"
    );
    assert_eq!(
        step_entity_count(&mirrored_text, "PLANE("),
        source_planes,
        "Mirror should preserve the source's PLANE( count exactly"
    );
    assert_eq!(
        step_entity_count(&mirrored_text, "B_SPLINE_SURFACE"),
        0,
        "Mirror should introduce no B_SPLINE_SURFACE entities"
    );
    let mirrored_volume = volume_of(&kernel, mirrored);
    let mirrored_rel_err = (mirrored_volume - source_volume).abs() / source_volume;
    assert!(
        mirrored_rel_err < 1e-12,
        "Mirror volume should be bit-exact vs source (measured identical to the last printed \
         digit), got rel_err={mirrored_rel_err:e}"
    );

    // AffineApply det<0 destroys analytic geometry and drifts the volume.
    let affine_text = step_text(&kernel, affine);
    assert_eq!(
        step_entity_count(&affine_text, "CYLINDRICAL_SURFACE"),
        0,
        "AffineApply det<0 should destroy the analytic CYLINDRICAL_SURFACE entirely \
         (BRepBuilderAPI_GTransform rewrites it as a B-spline). Characterization pin \
         (follow-up task #6652 / ticket tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC) — if this assertion \
         FAILS, OCCT has gotten BETTER at preserving analytic geometry under GTransform: \
         update this pin and this module's doc rather than treating the failure as a \
         regression."
    );
    assert_eq!(
        step_entity_count(&affine_text, "PLANE("),
        0,
        "AffineApply det<0 should destroy the analytic PLANE( entities entirely. \
         Characterization pin (follow-up task #6652 / ticket \
         tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC) — if this assertion FAILS, OCCT has gotten BETTER \
         at preserving analytic geometry under GTransform: update this pin and this module's \
         doc rather than treating the failure as a regression."
    );
    assert!(
        step_entity_count(&affine_text, "B_SPLINE_SURFACE") > 0,
        "AffineApply det<0 should introduce >=1 B_SPLINE_SURFACE entity (measured 5); \
         asserted as > 0 rather than an exact count since the entity name mixes plain and \
         complex-entity spellings. Characterization pin (follow-up task #6652 / ticket \
         tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC) — if this assertion FAILS, OCCT has gotten BETTER \
         at preserving analytic geometry under GTransform (no B-spline substitution needed): \
         update this pin and this module's doc rather than treating the failure as a \
         regression."
    );
    let affine_volume = volume_of(&kernel, affine);
    let affine_rel_err = (affine_volume - source_volume).abs() / source_volume;
    assert!(
        affine_rel_err > 1e-4,
        "AffineApply det<0 volume should differ from source by more than 1e-4 relative \
         (measured +8.615e-3) — this is a real analytic-to-B-spline approximation loss, not \
         noise, got rel_err={affine_rel_err:e}. Characterization pin (follow-up task #6652 / \
         ticket tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC) — if this assertion FAILS, OCCT has gotten \
         BETTER at preserving analytic geometry under GTransform (volume now survives \
         intact): update this pin and this module's doc rather than treating the failure as \
         a regression."
    );

    // --- Half (b): pre-tessellation fragility, determinant-independent ---
    let (mut kernel_b, base) = fresh_pretessellated_cylinder(1e-4);

    let mirrored_b = mirror_across_yz(&mut kernel_b, base);
    assert!(
        flag_of(&kernel_b, GeometryQuery::IsWatertight(mirrored_b)),
        "Mirror on a pre-tessellated source should remain IsWatertight=true — this is the \
         invariant A-δ (#6618) depends on, and it is the stable half of this test"
    );

    let affine_b = affine_reflect_x(&mut kernel_b, base);
    assert!(
        !flag_of(&kernel_b, GeometryQuery::IsWatertight(affine_b)),
        "Known defect (follow-up task #6652 / ticket tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC): \
         AffineApply det<0 on a pre-tessellated source is IsWatertight=false — \
         BRepTools_GTrsfModification carries the source's stale Poly_Triangulation across the \
         analytic-to-B-spline rewrite, so it no longer matches the new geometry and \
         BRepCheck_Analyzer::IsValid() fails. If this assertion FAILS, the defect has been \
         FIXED — delete this characterization pin, update this module's doc, and close #6652 \
         (ticket tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC)."
    );

    let identity_b = affine_linear(
        &mut kernel_b,
        base,
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    );
    assert!(
        !flag_of(&kernel_b, GeometryQuery::IsWatertight(identity_b)),
        "Known defect (follow-up task #6652 / ticket tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC): \
         determinant-independence proof — AffineApply with the IDENTITY linear map on a \
         pre-tessellated source is ALSO IsWatertight=false, proving this hazard belongs to \
         BRepBuilderAPI_GTransform itself and is NOT a det<0 orientation defect. If this \
         assertion FAILS, the defect has been FIXED — delete this characterization pin, \
         update this module's doc, and close #6652 (ticket \
         tkt_0RSXJ6CYZKF1B8Z31FWEMRF2GC)."
    );
}

// ---------------------------------------------------------------------------
// Pre-tessellation fragility helper
// ---------------------------------------------------------------------------

/// Build a FRESH `OcctKernel` containing only a fresh, untransformed
/// `cylinder_r6_h20` fixture (same dimensions as [`convex_fixtures`], via the
/// shared `CYLINDER_*` consts so the two can never drift apart), tessellate
/// it ONCE at `deflection`, and return the kernel plus the still-untransformed
/// handle.
///
/// The fresh kernel and the tessellate-before-reflect ordering are both
/// load-bearing:
///
///   - **Fresh kernel.** The pre-tessellation must apply to THIS test's
///     source handle only. Reusing a shared kernel/fixture would silently
///     contaminate the assertions in tests 1-3
///     (`both_reflection_paths_yield_valid_positive_volume_solids`,
///     `both_reflection_paths_tessellate_to_outward_wound_closed_manifold`,
///     `reflected_brep_step_export_bakes_geometry_and_emits_no_det_negative_placement`),
///     which all require an UNTESSELLATED source — tessellating a fixture
///     before reflecting it is exactly the hazard test 4 half (b) is
///     characterizing.
///   - **Tessellate-then-reflect ordering.** Verified 3-way in the probe:
///     (A) no pre-tessellation → both `Mirror` and `AffineApply` (det<0 and
///     identity) report `IsWatertight=true`; (B) pre-tessellate the SOURCE
///     (this helper's case) → `AffineApply` det<0 AND identity both report
///     `IsWatertight=false` while `Mirror` stays `true`; (C) tessellating the
///     RESULT after the transform is harmless. Only ordering (B) exposes the
///     `BRepTools_GTrsfModification` stale-`Poly_Triangulation` hazard that
///     test 4 half (b) pins.
fn fresh_pretessellated_cylinder(deflection: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let cyl_src = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(CYLINDER_RADIUS),
            height: Value::Real(CYLINDER_HEIGHT),
        })
        .expect("cylinder_r6_h20 should build");
    let base = kernel
        .execute(&GeometryOp::Translate {
            target: cyl_src.id,
            dx: CYLINDER_DX,
            dy: CYLINDER_DY,
            dz: 0.0,
        })
        .expect("cylinder_r6_h20 translate to x>0 should succeed")
        .id;

    kernel.tessellate(base, deflection).unwrap_or_else(|e| {
        panic!("pre-tessellation of cylinder_r6_h20 at {deflection:e} should succeed: {e:?}")
    });

    (kernel, base)
}
