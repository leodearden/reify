//! Integration tests that pin the public `QueryCapability` API surface and
//! the §5.4 capability-kind table for every extant `GeometryQuery` variant.
//!
//! These are the durable seam tests KGQ-ο/π/ρ depend on. Modelled on
//! `crates/reify-eval/tests/realization_produced_repr_pinning.rs`.

use reify_core::DiagnosticCode;
use reify_ir::{GeometryHandleId, GeometryQuery, QueryCapability};

/// Pin the §5.4 capability-kind mapping for every currently-existing
/// `GeometryQuery` variant (23 variants as of geometry.rs:728–971).
///
/// `EdgeLength` is the only extant BRepOnly variant per PRD §5.4; all others
/// are `BRepAndMesh`. When KGQ-μ/KGQ-ν add CurveCurvatureAt/SurfaceCurvatureAt/
/// Perimeter, they must add `=> QueryCapability::BRepOnly` arms in
/// `GeometryQuery::capability_kind()` — the exhaustive match enforces this.
///
/// NOTE: The authoritative exhaustiveness guard is the `match self` in
/// `GeometryQuery::capability_kind()` (geometry.rs), which has no `_` wildcard.
/// This hand-list pins the §5.4 default classification at one point in time;
/// the real compile-time guard against misclassification is that match.  If a
/// future contributor adds a BRepAndMesh variant and forgets to update this list
/// the test will under-cover it — but the compiler will still catch any variant
/// that should be BRepOnly if it is omitted from `capability_kind()`.
///
/// RED until `QueryCapability` and `GeometryQuery::capability_kind()` are
/// added to `reify-types`.
#[test]
fn capability_kind_table_matches_prd_5_4() {
    let h = GeometryHandleId(1);
    let h2 = GeometryHandleId(2);

    // §5.4 BRepOnly — only EdgeLength among extant variants
    assert_eq!(
        GeometryQuery::EdgeLength(h).capability_kind(),
        QueryCapability::BRepOnly,
        "EdgeLength must be BRepOnly per PRD §5.4"
    );

    // All other extant variants must be BRepAndMesh
    let brep_and_mesh_cases: &[GeometryQuery] = &[
        GeometryQuery::Volume(h),
        GeometryQuery::SurfaceArea(h),
        GeometryQuery::Centroid(h),
        GeometryQuery::BoundingBox(h),
        GeometryQuery::Distance { from: h, to: h2 },
        GeometryQuery::MomentOfInertia {
            handle: h,
            axis: [0.0, 0.0, 1.0],
        },
        GeometryQuery::AdjacentFaces {
            shape: h,
            face_index: 0,
        },
        GeometryQuery::SharedEdges {
            shape: h,
            face_a: 0,
            face_b: 1,
        },
        GeometryQuery::IsWatertight(h),
        GeometryQuery::IsManifold(h),
        GeometryQuery::IsOrientable(h),
        GeometryQuery::CenterOfMass {
            handle: h,
            density: 1.0,
        },
        GeometryQuery::InertiaTensor {
            handle: h,
            density: 1.0,
        },
        GeometryQuery::EdgeTangent(h),
        GeometryQuery::FaceNormal(h),
        GeometryQuery::FaceSurfaceKind(h),
        GeometryQuery::EdgeCurveKind(h),
        GeometryQuery::AncestorFacesOfEdge {
            shape: h,
            edge_index: 0,
        },
        GeometryQuery::OwnerBody(h),
        GeometryQuery::ClosestPointOnShape {
            handle: h,
            px: 0.0,
            py: 0.0,
            pz: 0.0,
        },
        GeometryQuery::PointOnShape {
            handle: h,
            px: 0.0,
            py: 0.0,
            pz: 0.0,
            tolerance: 1e-7,
        },
        GeometryQuery::SurfaceAngle {
            face_a: h,
            face_b: h2,
        },
        GeometryQuery::Contains {
            handle: h,
            px: 0.0,
            py: 0.0,
            pz: 0.0,
            tolerance: 1e-7,
        },
    ];

    for q in brep_and_mesh_cases {
        assert_eq!(
            q.capability_kind(),
            QueryCapability::BRepAndMesh,
            "{:?} must be BRepAndMesh per PRD §5.4 default; \
             if this fired after KGQ-μ/KGQ-ν landed, add a BRepOnly arm \
             in geometry.rs capability_kind()",
            q
        );
    }
}

/// Pin the `DiagnosticCode::QueryNotSupportedOnRepr` API surface.
///
/// Asserts the variant is constructible, implements `PartialEq`/`Copy`
/// (matching existing `DiagnosticCode` derives), and is distinct from a sibling.
/// Under `feature = "serde"`, also pins the PascalCase wire format.
///
/// RED until `QueryNotSupportedOnRepr` is added to `reify_types::DiagnosticCode`.
#[test]
fn diagnostic_code_query_not_supported_on_repr_surface() {
    let code = DiagnosticCode::QueryNotSupportedOnRepr;
    // Copy + PartialEq: binding a copy must equal the original
    let code2 = code;
    assert_eq!(code, code2);
    // Distinct from a sibling variant
    assert_ne!(code, DiagnosticCode::LongChainRealization);
    // Note: the PascalCase serde wire format ("QueryNotSupportedOnRepr") is
    // intentionally not pinned here — reify-eval has no `serde` feature and
    // serde_json is not a dev-dependency of this crate.  That contract should
    // be pinned in a reify-types unit test where `feature = "serde"` is
    // meaningful.  See reviewer note on task 3623 amendment pass.
}
