//! A-ε probe (task #6619, PRD `docs/prds/v0_6/assembly-derivation-toolbox.md`
//! leaf A-ε, boundary test T17): placeholder header. Replaced with the full
//! finding write-up in the doc-only step that lands last in this module's
//! plan.

#![cfg(has_occt)]

use reify_ir::{ExportFormat, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};

// ---------------------------------------------------------------------------
// Test 1 (steps 1-2) — det<0 reflection yields a valid, positive-volume solid
// ---------------------------------------------------------------------------

/// For each of three convex, primitive-derived fixtures, reflect across the
/// x=0 plane by BOTH lowerings — [`GeometryOp::Mirror`] (`gp_Trsf::SetMirror`)
/// and [`GeometryOp::AffineApply`] with `linear = diag(-1,1,1)`
/// (`gp_GTrsf` / `BRepBuilderAPI_GTransform`) — and assert that both paths
/// yield a BRepCheck-valid, positive-volume solid whose volume matches the
/// source within a path-specific tolerance.
///
/// RED: `convex_fixtures`, `mirror_across_yz`, `affine_reflect_x`,
/// `volume_of` and `flag_of` do not exist yet, so this fails to COMPILE
/// until step-2 adds them.
///
/// Fixtures are deliberately primitive-derived (never a boolean result: a
/// boolean returns a COMPOUND, and `IsWatertight` hard-returns `false` for
/// any non-SOLID/COMPSOLID/SHELL shape regardless of validity) and are NOT
/// tessellated before reflecting here — pre-tessellation ordering is step-7's
/// subject, and tessellating in this test would contaminate its answer.
///
/// Tolerances: `Mirror` is bit-exact against the source (measured identical
/// to the last printed digit, e.g. cylinder 2.261946710585e-6 m³ both
/// sides), so 1e-12 relative is used. `AffineApply` det<0 rewrites every
/// analytic surface as a B-spline approximation; measured worst-case drift
/// is +8.615e-3 relative (cylinder) and +8.250e-3 (cone), so 2e-2 relative
/// (≈2.3× the measured worst case) is used — tightening this to, say, 1e-9
/// would be a doomed assertion.
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

        let mirrored = mirror_across_yz(&mut kernel, source);
        let affine = affine_reflect_x(&mut kernel, source);

        for (path, target, tol) in [("Mirror", mirrored, 1e-12), ("AffineApply", affine, 2e-2)] {
            // (b) positive volume under reflection.
            let v = volume_of(&kernel, target);
            assert!(
                v > 0.0,
                "{name} via {path}: reflected volume must be positive, got {v:e}"
            );

            // (c)/(d) volume matches source within the path-specific tolerance.
            let rel_err = (v - source_volume).abs() / source_volume;
            assert!(
                rel_err < tol,
                "{name} via {path}: reflected volume {v:e} should match source {source_volume:e} \
                 within {tol:e} relative, got rel_err={rel_err:e}"
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
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (step-2) — kept PRIVATE to this module: they encode this
// probe's fixture contract, not a crate-wide idiom. `tests/common/mod.rs` is
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
///   - **Positioned wholly at x>0.** step-5's baked-geometry STEP assertion
///     reads a negative leading X coordinate in an exported `CARTESIAN_POINT`
///     entity as the reflection signal; a fixture straddling or left of x=0
///     would make that signal ambiguous.
///
/// All three are additionally CONVEX, which is what licenses the AABB-centre
/// reference direction in the outward-winding tessellation check (steps 3-4)
/// — a concave fixture can legitimately have inward-pointing dot products in
/// its concave regions (measured: 343 outward / 253 inward on a box-minus-
/// cylinder-minus-sphere part), which would make that assertion meaningless.
fn convex_fixtures(kernel: &mut OcctKernel) -> Vec<(&'static str, GeometryHandleId)> {
    let box_src = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.010),
            height: Value::Real(0.020),
            depth: Value::Real(0.030),
        })
        .expect("box_10x20x30 should build");
    let box_id = kernel
        .execute(&GeometryOp::Translate {
            target: box_src.id,
            dx: 0.050,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_10x20x30 translate to x>0 should succeed")
        .id;

    let cyl_src = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(0.006),
            height: Value::Real(0.020),
        })
        .expect("cylinder_r6_h20 should build");
    let cyl_id = kernel
        .execute(&GeometryOp::Translate {
            target: cyl_src.id,
            dx: 0.030,
            dy: 0.004,
            dz: 0.0,
        })
        .expect("cylinder_r6_h20 translate to x>0 should succeed")
        .id;

    let cone_src = kernel
        .execute(&GeometryOp::Cone {
            bottom_radius: Value::Real(0.008),
            top_radius: Value::Real(0.004),
            height: Value::Real(0.015),
        })
        .expect("cone_r8_r4_h15 should build");
    let cone_id = kernel
        .execute(&GeometryOp::Translate {
            target: cone_src.id,
            dx: 0.030,
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
/// both GTransform hazards (step-7).
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
/// `diag(-1,1,1)`; step-7 reuses this general form directly with the
/// IDENTITY `diag(1,1,1)` to prove its two GTransform hazards are
/// determinant-independent rather than reflection artifacts.
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
