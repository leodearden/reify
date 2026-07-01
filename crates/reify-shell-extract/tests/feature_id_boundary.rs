//! Cross-cutting boundary/regression gate for the structured `FeatureId`
//! codec contract on `ShellExtractionResult` (P1 ε, task #4810).
//!
//! This is the B+H integration gate over already-landed, green foundation
//! work: P1 β (`crates/reify-shell-extract/src/result.rs`, #4807). It is a
//! committed CHARACTERIZATION suite, not a RED-first driver — a failing row
//! here means a genuine cross-cutting contract regression in β, never a bug
//! to silently patch upstream from this file.
//!
//! Rows covered here (see `docs/prds/naming-convergence/P1-structured-featureid-feature-value.md`):
//! B6 and B8.
//!
//! Ownership notes (asserted through PUBLIC seams only — integration tests
//! cannot reach private items):
//! - B6 is asserted at the PUBLIC `PersistentlyCacheable::{serialize_to_writer,
//!   deserialize_from_reader}` seam on a `ShellExtractionResult` built via the
//!   public `::new`. This exercises the same fallible `topology_attribute_from_disk`
//!   parse path as the in-crate `topology_attribute_codec_round_trips_structured_feature_id`
//!   (a private-fn round-trip), making this a superset/public-seam confirmation
//!   of that pin — not a duplicate.
//! - B7 (corrupt-wire → `io::ErrorKind::InvalidData`) stays owned by the
//!   in-crate `topology_attribute_from_disk_rejects_corrupt_feature_id`: it
//!   requires hand-building a corrupt `TopologyAttributeOnDisk`, a private
//!   wire-shape mirror unreachable from `tests/`.

use reify_core::persistent_cache::PersistentlyCacheable;
use reify_ir::geometry::{FeatureId, ModEntry, Role, TopologyAttribute};
use reify_shell_extract::{
    MidSurfaceAttributes, MidSurfaceEdgeRecord, MidSurfaceMesh, SegmentationResult,
    ShellExtractionResult,
};

/// B8: `ShellExtractionResult::FORMAT_VERSION == 1` (NOT 2). Per the ratified
/// esc-4810-62 correction, β's `FeatureId::from_str` decode-strictness change
/// is decode-only and does not alter the wire bytes, so no version bump is
/// warranted. Mirrors the in-crate `shell_extraction_result_format_version_is_one`.
#[test]
fn b8_format_version_stays_one() {
    assert_eq!(
        <ShellExtractionResult as PersistentlyCacheable>::FORMAT_VERSION,
        1
    );
}

/// A minimal, length-consistent `MidSurfaceMesh` fixture (mirrors the
/// in-crate `matched_lengths_mesh` fixture shape: 3 vertices / 1 triangle /
/// 3 thickness entries).
fn matched_lengths_mesh() -> MidSurfaceMesh {
    MidSurfaceMesh {
        vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        triangles: vec![[0, 1, 2]],
        thickness: vec![1.0, 1.0, 1.0],
    }
}

/// An empty `SegmentationResult` whose per-vertex/per-triangle label slabs
/// match `mesh`'s lengths (mirrors the in-crate `empty_segmentation_for`).
fn empty_segmentation_for(mesh: &MidSurfaceMesh) -> SegmentationResult {
    SegmentationResult {
        regions: vec![],
        vertex_labels: vec![u32::MAX; mesh.vertices.len()],
        triangle_labels: vec![u32::MAX; mesh.triangles.len()],
    }
}

/// B6: the full public codec round-trip preserves a structured `FeatureId`
/// through `ShellExtractionResult::{serialize_to_writer, deserialize_from_reader}`
/// — covering both a bare `Realization` root and a `Derived` (mid-surface)
/// shape, including a `Derived` `mod_history[].splitting_feature_id`, in
/// both a face record and an edge record.
#[test]
fn b6_codec_round_trips_structured_feature_id() {
    let mesh = matched_lengths_mesh();
    let segmentation = empty_segmentation_for(&mesh);

    let realization_id = FeatureId::realization("Bracket", 2);
    let derived_id = FeatureId::derived_mid_surface(&FeatureId::realization("Housing", 5));

    let naming = MidSurfaceAttributes {
        face_records: vec![
            TopologyAttribute {
                feature_id: realization_id.clone(),
                role: Role::MidSurfaceFace,
                local_index: 0,
                user_label: Some("realization-face".to_string()),
                mod_history: vec![ModEntry {
                    splitting_feature_id: FeatureId::realization("Splitter", 1),
                    split_index: 9,
                }],
            },
            TopologyAttribute {
                feature_id: derived_id.clone(),
                role: Role::MidSurfaceFace,
                local_index: 1,
                user_label: None,
                mod_history: vec![],
            },
        ],
        edges: vec![MidSurfaceEdgeRecord {
            attribute: TopologyAttribute {
                feature_id: derived_id.clone(),
                role: Role::MidSurfaceEdge,
                local_index: 0,
                user_label: None,
                mod_history: vec![ModEntry {
                    splitting_feature_id: derived_id.clone(),
                    split_index: 0,
                }],
            },
            region_pair: (0, 1),
        }],
    };

    let original = ShellExtractionResult::new(mesh, segmentation, naming, 0, vec![])
        .expect("matched-length fixture must satisfy the constructor invariant");

    let mut buf: Vec<u8> = Vec::new();
    original
        .serialize_to_writer(&mut buf)
        .expect("serialize_to_writer must succeed");
    let decoded = ShellExtractionResult::deserialize_from_reader(&mut &buf[..])
        .expect("deserialize_from_reader must succeed on a well-formed buffer");

    // Face record 0: Realization root survives structurally, not just its
    // Display string.
    assert_eq!(
        decoded.naming.face_records[0].feature_id, realization_id,
        "face_records[0].feature_id must round-trip structurally"
    );
    assert_eq!(decoded.naming.face_records[0].feature_id.entity(), "Bracket");
    assert_eq!(decoded.naming.face_records[0].feature_id.index(), 2);

    // Face record 1: Derived(MidSurface) survives as the structured variant,
    // not collapsed onto its realization-root entity name.
    assert_eq!(
        decoded.naming.face_records[1].feature_id, derived_id,
        "face_records[1].feature_id must round-trip structurally"
    );
    assert_eq!(decoded.naming.face_records[1].feature_id.entity(), "Housing");
    assert_eq!(decoded.naming.face_records[1].feature_id.index(), 5);

    // Edge record: Derived feature_id AND its Derived mod_history entry both
    // survive.
    assert_eq!(
        decoded.naming.edges[0].attribute.feature_id, derived_id,
        "edges[0].attribute.feature_id must round-trip structurally"
    );
    assert_eq!(
        decoded.naming.edges[0].attribute.mod_history[0].splitting_feature_id, derived_id,
        "edges[0].attribute.mod_history[0].splitting_feature_id must round-trip structurally"
    );

    // Whole-struct equality (MidSurfaceAttributes/TopologyAttribute derive
    // Eq) confirms nothing else drifted alongside the feature_id fields.
    assert_eq!(decoded.naming, original.naming);
}
