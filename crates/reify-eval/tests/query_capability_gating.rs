//! Integration tests that pin the public `QueryCapability` API surface and
//! the §5.4 capability-kind table for every extant `GeometryQuery` variant.
//!
//! These are the durable seam tests KGQ-ο/π/ρ depend on. Modelled on
//! `crates/reify-eval/tests/realization_produced_repr_pinning.rs`.

use reify_types::{DiagnosticCode, GeometryHandleId, GeometryQuery, QueryCapability};

/// Pin the §5.4 capability-kind mapping for every currently-existing
/// `GeometryQuery` variant (23 variants as of geometry.rs:728–971).
///
/// `EdgeLength` is the only extant BRepOnly variant per PRD §5.4; all others
/// are `BRepAndMesh`. When KGQ-μ/KGQ-ν add CurveCurvatureAt/SurfaceCurvatureAt/
/// Perimeter, they must add `=> QueryCapability::BRepOnly` arms in
/// `GeometryQuery::capability_kind()` — the exhaustive match enforces this.
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
        GeometryQuery::MomentOfInertia { handle: h, axis: [0.0, 0.0, 1.0] },
        GeometryQuery::AdjacentFaces { shape: h, face_index: 0 },
        GeometryQuery::SharedEdges { shape: h, face_a: 0, face_b: 1 },
        GeometryQuery::IsWatertight(h),
        GeometryQuery::IsManifold(h),
        GeometryQuery::IsOrientable(h),
        GeometryQuery::CenterOfMass { handle: h, density: 1.0 },
        GeometryQuery::InertiaTensor { handle: h, density: 1.0 },
        GeometryQuery::EdgeTangent(h),
        GeometryQuery::FaceNormal(h),
        GeometryQuery::FaceSurfaceKind(h),
        GeometryQuery::EdgeCurveKind(h),
        GeometryQuery::AncestorFacesOfEdge { shape: h, edge_index: 0 },
        GeometryQuery::OwnerBody(h),
        GeometryQuery::ClosestPointOnShape { handle: h, px: 0.0, py: 0.0, pz: 0.0 },
        GeometryQuery::PointOnShape { handle: h, px: 0.0, py: 0.0, pz: 0.0, tolerance: 1e-7 },
        GeometryQuery::SurfaceAngle { face_a: h, face_b: h2 },
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

    #[cfg(feature = "serde")]
    {
        let s = serde_json::to_string(&code).unwrap();
        assert_eq!(s, r#""QueryNotSupportedOnRepr""#);
        let back: DiagnosticCode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, code);
    }
}
