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
}
