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
