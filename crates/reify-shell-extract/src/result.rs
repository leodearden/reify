//! Bundled producer-half output type for the shell-extract engine bridge
//! (PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` §2).
//!
//! [`ShellExtractionResult`] aggregates the five top-level outputs of the
//! shell-extract producer pipeline so that the future shell-extract
//! `ComputeNode` (task γ) has a single value type to persist via
//! `reify_eval::persistent_cache::PersistentlyCacheable`.
//!
//! # Wire-format precedent
//!
//! The encoding pipeline mirrors `reify_eval::persistent_cache::ElasticResult`
//! verbatim (see `crates/reify-eval/src/persistent_cache.rs:697-803`):
//! bincode 1.3 fixint-LE for the header, zstd 0.13 level-0 for the
//! compressed body, bytemuck LE casts for the f64/u32 slabs. Reusing the
//! precedent means every cacheable type in the engine shares one
//! wire-format contract — no per-type branching in the cache read/write path.
//!
//! # Length invariant
//!
//! `mid_surface.vertices.len() == mid_surface.thickness.len()` is enforced
//! at construction ([`ShellExtractionResult::new`]) and at deserialization
//! ([`PersistentlyCacheable::deserialize_from_reader`]). The PRD's literal
//! "`vertices.len() == 3 * thickness.len()`" wording assumes a flat-XYZ
//! `Vec<f64>` interpretation; under the structured `Vec<[f64;3]>` shape
//! used by [`crate::mid_surface::MidSurfaceMesh`], the structurally
//! equivalent invariant is one thickness per vertex.

use crate::mid_surface::MidSurfaceMesh;
use crate::mid_surface_naming::MidSurfaceAttributes;
use crate::segmentation::SegmentationResult;
use reify_types::diagnostics::Diagnostic;

/// Bundled producer-half output of the shell-extract pipeline.
///
/// Per PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` §2, this struct
/// packages the five top-level outputs the future shell-extract `ComputeNode`
/// (task γ) will persist via the
/// [`reify_eval::persistent_cache::PersistentlyCacheable`] trait.
///
/// Construct via [`ShellExtractionResult::new`] so the length invariant
/// `mid_surface.vertices.len() == mid_surface.thickness.len()` is enforced
/// at the producer boundary; direct field construction is also permitted
/// for callers that have already validated their inputs (e.g. test fixtures
/// driving the bit-exact round-trip pin).
///
/// **No `PartialEq` derive** because `reify_types::diagnostics::Diagnostic`
/// (one element type of `diagnostics: Vec<Diagnostic>`) does not impl
/// `PartialEq` at the struct level (only `Debug + Clone`). The round-trip
/// test compares fields individually (via `f64::to_bits()` for NaN/Inf
/// preservation, element-equal scans elsewhere), so a struct-level
/// `PartialEq` is not required.
#[derive(Debug, Clone)]
pub struct ShellExtractionResult {
    /// Mid-surface mesh produced by [`crate::extract_mid_surface`].
    pub mid_surface: MidSurfaceMesh,
    /// Per-region segmentation produced by [`crate::segment_regions`].
    pub segmentation: SegmentationResult,
    /// Topology-attribute records (face + edge) produced by
    /// [`crate::populate_mid_surface_attributes`].
    pub naming: MidSurfaceAttributes,
    /// Wall-clock cost of the producer pipeline in milliseconds. Surfaced
    /// to the persistent-cache layer for cost-weighted LRU eviction via
    /// [`reify_eval::persistent_cache::PersistentlyCacheable::solve_time_ms`].
    pub solve_time_ms: u64,
    /// Soft-warning diagnostics from the producer pipeline (e.g. clipped
    /// region, degenerate fan). Hard failures route through
    /// `ComputeOutcome::Failed` per task γ; this list never contains
    /// `Severity::Error`-level entries on a successful build.
    pub diagnostics: Vec<Diagnostic>,
}

/// Errors returned by [`ShellExtractionResult::new`] and by the
/// deserialization path when an invariant is violated.
///
/// `#[non_exhaustive]` lets future variants be added without breaking
/// external match exhaustiveness.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShellExtractionResultError {
    /// `mid_surface.thickness.len()` must equal `mid_surface.vertices.len()`.
    /// A mismatch indicates a caller-constructed (non-T2-produced) bundle
    /// with inconsistent parallel arrays. Shape mirrors
    /// [`crate::segmentation::SegmentationError::MeshLengthMismatch`] so
    /// the contract is uniform across the crate.
    ///
    /// The PRD's literal "`vertices.len() == 3 * thickness.len()`" wording
    /// is the flat-XYZ-coordinate interpretation; under the structured
    /// `Vec<[f64; 3]>` shape used by
    /// [`crate::mid_surface::MidSurfaceMesh`], the structurally equivalent
    /// invariant is one thickness per vertex.
    LengthInvariantViolation {
        /// Number of vertices in the bundled mid-surface mesh.
        vertices_len: usize,
        /// Number of thickness entries in the bundled mid-surface mesh.
        thickness_len: usize,
    },
}

impl std::fmt::Display for ShellExtractionResultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellExtractionResultError::LengthInvariantViolation {
                vertices_len,
                thickness_len,
            } => write!(
                f,
                "mid_surface.thickness.len() ({thickness_len}) ≠ \
                 mid_surface.vertices.len() ({vertices_len}); \
                 the two parallel arrays must be the same length"
            ),
        }
    }
}

impl std::error::Error for ShellExtractionResultError {}

impl ShellExtractionResult {
    /// Construct a `ShellExtractionResult`, enforcing the length invariant
    /// `mid_surface.vertices.len() == mid_surface.thickness.len()`.
    ///
    /// Returns [`ShellExtractionResultError::LengthInvariantViolation`] if
    /// the two parallel arrays disagree. Producer code (the future
    /// shell-extract `ComputeNode`, task γ) should always route through
    /// this constructor; test fixtures that need to bypass the check may
    /// use direct field construction.
    pub fn new(
        mid_surface: MidSurfaceMesh,
        segmentation: SegmentationResult,
        naming: MidSurfaceAttributes,
        solve_time_ms: u64,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Self, ShellExtractionResultError> {
        if mid_surface.vertices.len() != mid_surface.thickness.len() {
            return Err(ShellExtractionResultError::LengthInvariantViolation {
                vertices_len: mid_surface.vertices.len(),
                thickness_len: mid_surface.thickness.len(),
            });
        }
        Ok(Self {
            mid_surface,
            segmentation,
            naming,
            solve_time_ms,
            diagnostics,
        })
    }
}

// Compile-time assertion that `ShellExtractionResult: PersistentlyCacheable`.
// Lives at module scope (outside `#[cfg(test)]`) so the trait-bound is
// enforced on every build, not only when `cargo test` links. Mirrors
// `persistent_cache.rs:369-372`.
const _: fn() = || {
    fn assert_impl<T: reify_eval::persistent_cache::PersistentlyCacheable>() {}
    assert_impl::<ShellExtractionResult>();
};

impl reify_eval::persistent_cache::PersistentlyCacheable for ShellExtractionResult {
    const FORMAT_VERSION: u32 = 1;

    fn serialize_to_writer(&self, _w: &mut impl std::io::Write) -> std::io::Result<()> {
        unimplemented!("step-6 wires this up")
    }

    fn deserialize_from_reader(_r: &mut impl std::io::Read) -> std::io::Result<Self> {
        unimplemented!("step-6 wires this up")
    }

    fn uncompressed_byte_size(&self) -> u64 {
        unimplemented!("step-12 wires this up")
    }

    fn solve_time_ms(&self) -> u64 {
        self.solve_time_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mid_surface::MidSurfaceMesh;
    use crate::mid_surface_naming::MidSurfaceAttributes;
    use crate::segmentation::SegmentationResult;
    use reify_eval::persistent_cache::PersistentlyCacheable;

    #[test]
    fn shell_extraction_result_public_surface_is_callable() {
        // (c) smoke-construct with empty inner buffers. Direct field
        // construction is permitted for test fixtures; production callers
        // should prefer `ShellExtractionResult::new` (step-4) to get the
        // length-invariant check.
        let value = ShellExtractionResult {
            mid_surface: MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            segmentation: SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            naming: MidSurfaceAttributes::default(),
            solve_time_ms: 0,
            diagnostics: vec![],
        };

        // (a) destructure trip-wire: if any field is added/removed/renamed
        // upstream, this destructure will fail to compile. Production code
        // should never rely on destructure-completeness; this is solely a
        // test-side regression detector for PRD §2 drift.
        let ShellExtractionResult {
            mid_surface,
            segmentation,
            naming,
            solve_time_ms,
            diagnostics,
        } = value;
        assert!(mid_surface.vertices.is_empty());
        assert!(segmentation.regions.is_empty());
        assert!(naming.face_records.is_empty());
        assert_eq!(solve_time_ms, 0);
        assert!(diagnostics.is_empty());

        // (b) the type is re-exported from the crate root. If `pub use
        // result::ShellExtractionResult` is removed from `lib.rs`, this
        // path will fail to resolve at compile time.
        let _from_root: crate::ShellExtractionResult = ShellExtractionResult {
            mid_surface: MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            segmentation: SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            naming: MidSurfaceAttributes::default(),
            solve_time_ms: 0,
            diagnostics: vec![],
        };
    }

    #[test]
    fn shell_extraction_result_format_version_is_one() {
        // Read the FORMAT_VERSION associated const directly — no instance
        // needed, demonstrating the cache-layer use case where `(TypeId,
        // FORMAT_VERSION)` can be looked up before any value materialises.
        // Pins the project convention that FORMAT_VERSION starts at 1
        // because 0 means "uninitialised / unknown" — mirrors
        // `elastic_result_format_version_is_one` at
        // `persistent_cache.rs:1101`.
        assert_eq!(
            <ShellExtractionResult as PersistentlyCacheable>::FORMAT_VERSION,
            1
        );
    }

    /// Build a fresh `MidSurfaceMesh` with `vertices.len() == thickness.len()`
    /// to use as a known-good fixture for the constructor pin tests.
    fn matched_lengths_mesh() -> MidSurfaceMesh {
        MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0, 1.0],
        }
    }

    fn empty_segmentation_for(mesh: &MidSurfaceMesh) -> SegmentationResult {
        SegmentationResult {
            regions: vec![],
            vertex_labels: vec![u32::MAX; mesh.vertices.len()],
            triangle_labels: vec![u32::MAX; mesh.triangles.len()],
        }
    }

    #[test]
    fn shell_extraction_result_new_rejects_length_invariant_violation() {
        // Construct a deliberately mismatched mesh: 3 vertices vs 2 thicknesses.
        // The constructor must reject with the parallel-array length pair —
        // mirroring `SegmentationError::MeshLengthMismatch`'s shape so the
        // contract is uniform across the crate.
        let bad_mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0],
        };
        let segmentation = empty_segmentation_for(&bad_mesh);
        let err = ShellExtractionResult::new(
            bad_mesh,
            segmentation,
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .expect_err("3 vertices vs 2 thicknesses must violate the length invariant");
        assert_eq!(
            err,
            ShellExtractionResultError::LengthInvariantViolation {
                vertices_len: 3,
                thickness_len: 2,
            }
        );
    }

    #[test]
    fn shell_extraction_result_new_accepts_matching_lengths() {
        // Matched lengths (3 == 3) must succeed.
        let good = ShellExtractionResult::new(
            matched_lengths_mesh(),
            empty_segmentation_for(&matched_lengths_mesh()),
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .expect("matched parallel-array lengths must be accepted");
        assert_eq!(good.mid_surface.vertices.len(), good.mid_surface.thickness.len());

        // The empty-mesh case (0 == 0) is also a legal match.
        let empty_mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let empty = ShellExtractionResult::new(
            empty_mesh,
            SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .expect("empty-mesh case (0 == 0) is a legal match");
        assert!(empty.mid_surface.vertices.is_empty());
    }

    #[test]
    fn shell_extraction_result_solve_time_ms_returns_constructor_value() {
        // Build two values with distinct solve_time_ms and confirm the trait
        // accessor reads the field. Mirrors `elastic_result_solve_time_ms_*`
        // at `persistent_cache.rs:1115` — the second sample with
        // solve_time_ms = 0 catches a hard-coded constant.
        let nine_thousand_nine_hundred_ninety_nine = ShellExtractionResult::new(
            MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            MidSurfaceAttributes::default(),
            9999,
            vec![],
        )
        .unwrap();
        assert_eq!(nine_thousand_nine_hundred_ninety_nine.solve_time_ms(), 9999);

        let zero = ShellExtractionResult::new(
            MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .unwrap();
        assert_eq!(zero.solve_time_ms(), 0);
    }

    // ---- step-5 round-trip pins ----

    use crate::mid_surface_naming::MidSurfaceEdgeRecord;
    use crate::segmentation::{RegionClassification, RegionInfo};
    use reify_types::diagnostics::{Diagnostic, DiagnosticLabel, Severity, SourceSpan};
    use reify_types::geometry::{CapKind, FeatureId, ModEntry, Role, TopologyAttribute};

    /// Build a non-trivial `ShellExtractionResult` exercising every wire-format
    /// path: special-value f64 components (NaN / ±Inf / -0.0), non-empty u32
    /// label slabs, per-region voxel slabs, region metric f64::INFINITY, a
    /// `TopologyAttribute` with `Cap(Top)` role + `user_label = Some(...)` +
    /// non-empty `mod_history`, a `MidSurfaceEdgeRecord` with `MidSurfaceEdge`
    /// role + `user_label = Some(...)`, and a `Diagnostic` carrying a
    /// `DiagnosticLabel` + non-empty `candidates`.
    fn make_round_trip_fixture() -> ShellExtractionResult {
        let mid_surface = MidSurfaceMesh {
            vertices: vec![
                [f64::NAN, 0.0, -0.0],
                [f64::INFINITY, f64::NEG_INFINITY, std::f64::consts::PI],
                [1.0, 2.0, 3.0],
            ],
            triangles: vec![[0, 1, 2], [2, 1, 0]],
            thickness: vec![f64::NAN, 1.5, 0.0],
        };
        let region_a = RegionInfo {
            label: 0,
            voxels: vec![[0, 0, 0], [1, 0, 0], [2, 0, 0]],
            mean_thickness: 0.25,
            extent: 1.0,
            thickness_extent_ratio: 0.25,
            classification: RegionClassification::ShellEligible,
        };
        let region_b = RegionInfo {
            label: 1,
            voxels: vec![[5, 5, 5]],
            mean_thickness: 1.0,
            extent: 0.0,
            thickness_extent_ratio: f64::INFINITY,
            classification: RegionClassification::TetEligible,
        };
        let segmentation = SegmentationResult {
            regions: vec![region_a, region_b],
            vertex_labels: vec![0, 1, u32::MAX],
            triangle_labels: vec![0, u32::MAX],
        };
        let parent = FeatureId::new("Bracket#realization[0]");
        let derived = FeatureId::derived_mid_surface(&parent);
        let face_record = TopologyAttribute {
            feature_id: derived.clone(),
            role: Role::Cap(CapKind::Top),
            local_index: 7,
            user_label: Some("user-face".to_string()),
            mod_history: vec![ModEntry {
                splitting_feature_id: parent.clone(),
                split_index: 3,
            }],
        };
        let edge_attr = TopologyAttribute {
            feature_id: derived,
            role: Role::MidSurfaceEdge,
            local_index: 0,
            user_label: Some("user-edge".to_string()),
            mod_history: vec![],
        };
        let edge_record = MidSurfaceEdgeRecord {
            attribute: edge_attr,
            region_pair: (0, 1),
        };
        let naming = MidSurfaceAttributes {
            face_records: vec![face_record],
            edges: vec![edge_record],
        };
        let diagnostic = Diagnostic::warning("clipped mid-surface near domain boundary")
            .with_label(DiagnosticLabel::new(
                SourceSpan::new(10, 25),
                "diagnostic-label-message",
            ))
            .with_candidates(vec!["cand-a".to_string(), "cand-b".to_string()]);
        ShellExtractionResult {
            mid_surface,
            segmentation,
            naming,
            solve_time_ms: 4242,
            diagnostics: vec![diagnostic],
        }
    }

    /// Assert that two `f64` values share the exact same bit-pattern. Used
    /// for NaN / Inf / signed-zero round-trip pinning where `PartialEq`
    /// would fail (NaN != NaN) or silently accept sign-bit drift. Mirrors
    /// `persistent_cache.rs:1192` (the ElasticResult NaN bit-pattern pin).
    fn assert_f64_bits_eq(label: &str, a: f64, b: f64) {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{label}: bit-pattern drift (a={a:?}, b={b:?})"
        );
    }

    #[test]
    fn shell_extraction_result_serialize_deserialize_round_trip_preserves_all_buffers_bit_exact() {
        let original = make_round_trip_fixture();
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ShellExtractionResult::deserialize_from_reader(&mut &buf[..]).unwrap();

        // (a) every f64 in mid_surface.vertices round-trips bit-exact.
        assert_eq!(
            decoded.mid_surface.vertices.len(),
            original.mid_surface.vertices.len(),
            "vertices length drift"
        );
        for (i, (d, o)) in decoded
            .mid_surface
            .vertices
            .iter()
            .zip(original.mid_surface.vertices.iter())
            .enumerate()
        {
            for (axis, (dv, ov)) in d.iter().zip(o.iter()).enumerate() {
                assert_f64_bits_eq(&format!("vertices[{i}][{axis}]"), *dv, *ov);
            }
        }

        // (b) every f64 in mid_surface.thickness round-trips bit-exact.
        assert_eq!(
            decoded.mid_surface.thickness.len(),
            original.mid_surface.thickness.len(),
            "thickness length drift"
        );
        for (i, (d, o)) in decoded
            .mid_surface
            .thickness
            .iter()
            .zip(original.mid_surface.thickness.iter())
            .enumerate()
        {
            assert_f64_bits_eq(&format!("thickness[{i}]"), *d, *o);
        }

        // (c) triangles, vertex_labels, triangle_labels round-trip element-equal.
        assert_eq!(decoded.mid_surface.triangles, original.mid_surface.triangles);
        assert_eq!(
            decoded.segmentation.vertex_labels,
            original.segmentation.vertex_labels
        );
        assert_eq!(
            decoded.segmentation.triangle_labels,
            original.segmentation.triangle_labels
        );

        // (d) per-region voxels round-trip element-equal.
        // (e) per-region f64 metrics round-trip bit-exact (incl. INFINITY).
        assert_eq!(
            decoded.segmentation.regions.len(),
            original.segmentation.regions.len()
        );
        for (i, (d, o)) in decoded
            .segmentation
            .regions
            .iter()
            .zip(original.segmentation.regions.iter())
            .enumerate()
        {
            assert_eq!(d.label, o.label, "region[{i}].label");
            assert_eq!(d.voxels, o.voxels, "region[{i}].voxels");
            assert_f64_bits_eq(
                &format!("region[{i}].mean_thickness"),
                d.mean_thickness,
                o.mean_thickness,
            );
            assert_f64_bits_eq(&format!("region[{i}].extent"), d.extent, o.extent);
            assert_f64_bits_eq(
                &format!("region[{i}].thickness_extent_ratio"),
                d.thickness_extent_ratio,
                o.thickness_extent_ratio,
            );
            assert_eq!(
                d.classification, o.classification,
                "region[{i}].classification"
            );
        }

        // (f) solve_time_ms round-trips equal.
        assert_eq!(decoded.solve_time_ms, original.solve_time_ms);

        // (g) naming.face_records and naming.edges round-trip equal under PartialEq.
        assert_eq!(decoded.naming.face_records, original.naming.face_records);
        assert_eq!(decoded.naming.edges, original.naming.edges);

        // (h) diagnostics: severity + message + candidates + labels round-trip;
        // code is documented-lossy → None on round-trip.
        assert_eq!(decoded.diagnostics.len(), original.diagnostics.len());
        for (i, (d, o)) in decoded
            .diagnostics
            .iter()
            .zip(original.diagnostics.iter())
            .enumerate()
        {
            assert_eq!(d.severity, o.severity, "diagnostics[{i}].severity");
            assert_eq!(d.message, o.message, "diagnostics[{i}].message");
            assert_eq!(d.candidates, o.candidates, "diagnostics[{i}].candidates");
            assert_eq!(
                d.labels.len(),
                o.labels.len(),
                "diagnostics[{i}].labels length"
            );
            for (j, (dl, ol)) in d.labels.iter().zip(o.labels.iter()).enumerate() {
                assert_eq!(dl.span, ol.span, "diagnostics[{i}].labels[{j}].span");
                assert_eq!(dl.message, ol.message, "diagnostics[{i}].labels[{j}].message");
            }
            // `code` field is documented as lossy: shell-specific
            // DiagnosticCode variants don't exist yet (task ε owns them),
            // so the wire-shape mirror drops it. Round-trips to None.
            assert_eq!(d.code, None, "diagnostics[{i}].code must round-trip to None");
        }
    }

    #[test]
    fn shell_extraction_result_round_trips_with_empty_buffers() {
        // Pin that all-zero-length slabs and empty nested Vecs round-trip
        // cleanly — the slab loops must not assume "at least one element"
        // (cf. `elastic_result_round_trips_with_empty_field_arrays` at
        // `persistent_cache.rs:1207`).
        let original = ShellExtractionResult {
            mid_surface: MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            segmentation: SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            naming: MidSurfaceAttributes::default(),
            solve_time_ms: 0,
            diagnostics: vec![],
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ShellExtractionResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        assert!(decoded.mid_surface.vertices.is_empty());
        assert!(decoded.mid_surface.triangles.is_empty());
        assert!(decoded.mid_surface.thickness.is_empty());
        assert!(decoded.segmentation.regions.is_empty());
        assert!(decoded.segmentation.vertex_labels.is_empty());
        assert!(decoded.segmentation.triangle_labels.is_empty());
        assert!(decoded.naming.face_records.is_empty());
        assert!(decoded.naming.edges.is_empty());
        assert_eq!(decoded.solve_time_ms, 0);
        assert!(decoded.diagnostics.is_empty());
    }
}
